//! Gmail driver tests (blueprint §6 acceptance) — **no live Gmail, no network, no credentials**.
//! Every test drives the introspective `Driver` surface and the apply leg against an in-memory
//! [`MockGmailClient`] (scripted Gmail fixtures + recorded calls), so we assert request shape +
//! response decoding + plan shape + token-safety without touching a socket.

use std::sync::Arc;

use qfs_driver::{check_capability, resolve_proc, Archetype, Driver, Path, Verb, VersionSupport};
use qfs_plan::{
    preview, DriverId, EffectKind, EffectNode, NodeId, PlanBuilder, ProcId, Target, VfsPath,
};
use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};
use qfs_types::{Column, Row, RowBatch, Schema, Value};

use super::*;

// ---- fixtures ----------------------------------------------------------------------------

/// A driver over a mock seeded with two labels and one message (with one attachment).
fn driver_with_mock() -> (GmailDriver, Arc<MockGmailClient>) {
    let mock = Arc::new(
        MockGmailClient::new()
            .with_labels(vec!["INBOX".to_string(), "SENT".to_string()])
            .with_message(MailMessage {
                id: "m1".to_string(),
                thread_id: "t1".to_string(),
                label_ids: vec!["INBOX".to_string()],
                date: 1_700_000_000_000,
                from: "alice@example.com".to_string(),
                subject: "hello".to_string(),
                snippet: "hi there".to_string(),
                message_id: "<parent-msgid@mail.gmail.com>".to_string(),
                attachments: vec![AttachmentMeta {
                    filename: "a.pdf".to_string(),
                    mime: "application/pdf".to_string(),
                    attachment_id: "att1".to_string(),
                    size: 42,
                }],
            }),
    );
    let driver = GmailDriver::new(mock.clone() as Arc<dyn GmailClient>);
    (driver, mock)
}

fn target(path: &str) -> Target {
    Target::new(DriverId::new("mail"), VfsPath::new(path))
}

/// Build a single-row draft args batch (the columns the effect decoder reads).
fn draft_args(cols: &[(&str, Value)]) -> RowBatch {
    let schema = Schema::new(
        cols.iter()
            .map(|(n, v)| Column::new(*n, v.type_of(), true))
            .collect(),
    );
    let row = Row::new(cols.iter().map(|(_, v)| v.clone()).collect());
    RowBatch::new(schema, vec![row])
}

// ---- introspection: mount / archetype / schema / version --------------------------------

#[test]
fn mount_and_id_are_mail() {
    let (d, _) = driver_with_mock();
    assert_eq!(d.mount(), "/mail");
    assert_eq!(d.id(), DriverId::new("mail"));
}

#[test]
fn describe_emits_append_archetype_and_message_schema() {
    let (d, _) = driver_with_mock();
    let desc = d.describe(&Path::new("/mail/INBOX")).unwrap();
    assert_eq!(desc.archetype, Archetype::AppendLog);
    // The canonical typed message columns.
    for col in [
        "id",
        "thread_id",
        "date",
        "from",
        "subject",
        "snippet",
        "label_ids",
        "attachments",
    ] {
        assert!(desc.schema.column(col).is_some(), "missing column {col}");
    }
    assert_eq!(
        desc.schema.column("date").unwrap().ty,
        qfs_types::ColumnType::Timestamp
    );
    assert_eq!(
        d.version_support(&Path::new("/mail/INBOX")),
        VersionSupport::None
    );
}

#[test]
fn describe_root_reports_the_label_listing_schema() {
    // The mailbox ROOT lists labels (`ls /mail`), so its DESCRIBE reports the `name` label schema —
    // not the message schema — keeping introspection honest about what a root read returns.
    let (d, _) = driver_with_mock();
    let desc = d.describe(&Path::new("/mail")).unwrap();
    assert_eq!(desc.archetype, Archetype::AppendLog);
    assert_eq!(desc.schema.columns.len(), 1);
    assert!(
        desc.schema.column("name").is_some(),
        "root lists labels by name"
    );
}

#[test]
fn describe_marks_the_label_tree_navigable_and_the_message_nodes_leaves() {
    // §9 enumerable-children conformance (slice 2): the mailbox is a NAVIGABLE label tree — `/mail`'s
    // children are labels and a label's are messages, both LOCATIONS (a message addresses further:
    // `/mail/<label>/<msg>/<att>`), so `cd /mail` then `cd INBOX` works. The shipped gate refused
    // both because it keyed on the archetype pair, and mail is an AppendLog.
    let (d, _) = driver_with_mock();
    assert!(d.describe(&Path::new("/mail")).unwrap().navigable);
    assert!(d.describe(&Path::new("/mail/INBOX")).unwrap().navigable);

    // The archetype stays AppendLog throughout — mail rows ARE an append log, and `ls` is
    // archetype-typed (§5.1), so calling the root a BlobNamespace would make `ls /mail` lower to the
    // blob name/size/is_dir/modified projection and fail against the label schema. Navigability is
    // an ORTHOGONAL fact about a node's children, which is why it is its own field.
    assert_eq!(
        d.describe(&Path::new("/mail")).unwrap().archetype,
        Archetype::AppendLog
    );

    // A message and its nested nodes are leaves: their children are rows/bytes, not locations.
    assert!(!d.describe(&Path::new("/mail/INBOX/m1")).unwrap().navigable);
    assert!(
        !d.describe(&Path::new("/mail/INBOX/m1/att1"))
            .unwrap()
            .navigable
    );
    assert!(!d.describe(&Path::new("/mail/drafts")).unwrap().navigable);
}

#[test]
fn describe_attachment_reports_the_attachment_read_schema() {
    // An attachment node (`/mail/<label>/<msg>/<att>`) reads ONE file's bytes: DESCRIBE must
    // advertise the same columns the scan returns (filename/mime/size/content), NOT the message
    // listing schema. Without this arm a cross-service
    // `SELECT filename AS name, mime AS mime_type, content AS bytes FROM /mail/<msg>/<att>` cannot
    // resolve its columns at plan time — the columns are shaped to line up with the Drive upload
    // row shape for a one-statement attachment → Drive-folder transfer.
    let (d, _) = driver_with_mock();
    let desc = d.describe(&Path::new("/mail/INBOX/m1/att1")).unwrap();
    for col in ["filename", "mime", "size", "content"] {
        assert!(
            desc.schema.column(col).is_some(),
            "attachment describe missing column {col}"
        );
    }
    assert_eq!(
        desc.schema.column("content").unwrap().ty,
        qfs_types::ColumnType::Bytes,
        "the attachment bytes column is the Drive upload payload"
    );
    // It must NOT be the message listing schema (no top-level `subject`/`attachments`).
    assert!(desc.schema.column("subject").is_none());
    assert!(desc.schema.column("attachments").is_none());
}

// ---- capability golden (path-keyed gate) -------------------------------------------------

#[test]
fn capabilities_are_path_keyed() {
    let (d, _) = driver_with_mock();

    // A label: select/update/remove, but NOT insert/upsert.
    let label = Path::new("/mail/INBOX");
    assert!(check_capability(&d, &label, Verb::Select).is_ok());
    assert!(check_capability(&d, &label, Verb::Update).is_ok());
    assert!(check_capability(&d, &label, Verb::Remove).is_ok());
    assert!(check_capability(&d, &label, Verb::Insert).is_err());

    // Drafts: insert/upsert/select, but NOT update, and NOT collection-level remove — a set-wide
    // `remove /mail/drafts where …` is unsafe (Gmail's lossy search), and single-draft discard is a
    // named follow-up, so the collection advertises no Remove.
    let drafts = Path::new("/mail/drafts");
    assert!(check_capability(&d, &drafts, Verb::Insert).is_ok());
    assert!(check_capability(&d, &drafts, Verb::Upsert).is_ok());
    let err = check_capability(&d, &drafts, Verb::Update).unwrap_err();
    assert_eq!(err.code(), "unsupported_verb");
    let rm = check_capability(&d, &drafts, Verb::Remove).unwrap_err();
    assert_eq!(rm.code(), "unsupported_verb");

    // A message: select + update (relabel the single message) + remove (trash), not insert. A
    // single message relabels directly via its node (ticket 20260704155500).
    let msg = Path::new("id:m1");
    assert!(check_capability(&d, &msg, Verb::Select).is_ok());
    assert!(check_capability(&d, &msg, Verb::Update).is_ok());
    assert!(check_capability(&d, &msg, Verb::Remove).is_ok());
    assert!(check_capability(&d, &msg, Verb::Insert).is_err());

    // A single draft (`/mail/drafts/<draft-id>`) is a DRAFT node (addressed by draft id, the
    // sendable identity), not a message node: it reads (`Select`) and sends (the `mail.send` CALL,
    // not a verb cap). It does NOT advertise a message trash — a message-id trash breaks on a draft
    // id, so draft discard (`drafts.delete`) is a named follow-up rather than a wrong `Remove`.
    let draft = Path::new("/mail/drafts/d1");
    assert!(check_capability(&d, &draft, Verb::Select).is_ok());
    assert_eq!(
        check_capability(&d, &draft, Verb::Remove)
            .unwrap_err()
            .code(),
        "unsupported_verb"
    );
}

#[test]
fn a_label_segment_is_passed_through_verbatim() {
    use crate::path::MailPath;
    // qfs does NOT normalize the label case — the segment is the label name verbatim (`inbox` stays
    // `inbox`). It reaches Gmail as a `label:<name>` SEARCH term, which Gmail matches
    // case-insensitively, so `/mail/inbox` reads the inbox without any qfs-side canonicalization.
    assert_eq!(
        MailPath::parse_str("/mail/inbox").unwrap(),
        MailPath::Label {
            name: "inbox".to_string()
        }
    );
    assert_eq!(
        MailPath::parse_str("/mail/Receipts").unwrap(),
        MailPath::Label {
            name: "Receipts".to_string()
        }
    );
}

#[test]
fn insert_into_message_is_rejected_structurally() {
    let (d, _) = driver_with_mock();
    let err = check_capability(&d, &Path::new("id:m1"), Verb::Insert).unwrap_err();
    match &err {
        qfs_driver::CfsError::UnsupportedVerb {
            verb, supported, ..
        } => {
            assert_eq!(*verb, "INSERT");
            assert!(supported.contains(&"SELECT"));
            assert!(supported.contains(&"REMOVE"));
        }
        other => panic!("expected UnsupportedVerb, got {other:?}"),
    }
}

// ---- procedures + prelude (mail.send irreversible; SEND alias) ---------------------------

#[test]
fn mail_send_is_declared_irreversible_with_compose_scope() {
    let (d, _) = driver_with_mock();
    let send = resolve_proc(&d, "send").unwrap();
    assert!(send.irreversible, "mail.send must be irreversible");
    assert_eq!(send.requires_scopes, vec![GMAIL_COMPOSE_SCOPE.to_string()]);
    // An undeclared CALL is rejected structurally.
    assert_eq!(
        resolve_proc(&d, "delete_forever").unwrap_err().code(),
        "unknown_procedure"
    );
}

#[test]
fn prelude_ships_send_alias_desugaring_to_mail_send() {
    let (d, _) = driver_with_mock();
    let alias = d.prelude().iter().find(|a| a.name == "SEND").unwrap();
    assert_eq!(alias.desugars_to, "mail.send");
}

// ---- pushdown: WHERE → Gmail q= ----------------------------------------------------------

#[test]
fn where_lowers_to_gmail_query_with_residual_kept_local() {
    use qfs_types::{CmpOp, ColRef, Literal, Predicate};
    // from = 'x@y' AND subject = 'hello' AND is_unread = true.
    // All three q= terms are pushed as a pre-filter, but `from`/`subject` are LOSSY (Gmail's
    // `from:`/`subject:` are address/substring matches, looser than SQL `=`), so they must be
    // KEPT as residual for exact local re-checking. Only the exact `is_unread` (→ is:unread)
    // drops out of the residual. Over-fetch then filter — never wrong rows (blueprint §7).
    let from_eq = Predicate::Cmp(
        ColRef::col("from"),
        CmpOp::Eq,
        Literal::Text("x@y".to_string()),
    );
    let subject_eq = Predicate::Cmp(
        ColRef::col("subject"),
        CmpOp::Eq,
        Literal::Text("hello".to_string()),
    );
    let pred = Predicate::And(
        Box::new(from_eq.clone()),
        Box::new(Predicate::And(
            Box::new(subject_eq.clone()),
            Box::new(Predicate::Cmp(
                ColRef::col("is_unread"),
                CmpOp::Eq,
                Literal::Bool(true),
            )),
        )),
    );
    let res = query::build_query(Some("INBOX"), Some(&pred));
    assert_eq!(res.query, "label:INBOX from:x@y subject:hello is:unread");
    // The residual re-checks the two lossy conjuncts (from = 'x@y' AND subject = 'hello'); the
    // exact `is_unread` is fully expressed by `is:unread` and is not re-checked.
    assert_eq!(
        res.residual,
        Some(Predicate::And(Box::new(from_eq), Box::new(subject_eq),)),
        "lossy from/subject equality is kept as residual; only exact is_unread drops out"
    );

    // An OR stays residual (Gmail term-ANDing cannot express it) — combined locally.
    let or_pred = Predicate::Or(
        Box::new(Predicate::Cmp(
            ColRef::col("from"),
            CmpOp::Eq,
            Literal::Text("a".into()),
        )),
        Box::new(Predicate::Cmp(
            ColRef::col("from"),
            CmpOp::Eq,
            Literal::Text("b".into()),
        )),
    );
    let res2 = query::build_query(Some("INBOX"), Some(&or_pred));
    assert_eq!(res2.query, "label:INBOX", "only the label scope pushed");
    assert!(res2.residual.is_some(), "OR is residual, filtered locally");

    // The driver declares partial WHERE+LIMIT pushdown.
    let (d, _) = driver_with_mock();
    assert!(d.pushdown().supports_where());
    assert!(d.pushdown().supports_limit());
    assert!(!d.pushdown().supports_order());
}

#[test]
fn exact_predicates_push_fully_with_no_residual() {
    use qfs_types::{CmpOp, ColRef, Literal, Predicate};
    // label = 'INBOX' AND is_unread = false: both map to Gmail terms that mean EXACTLY the SQL
    // predicate (exact label membership / exact read-state), so the whole predicate is pushed
    // and NOTHING is left to re-check locally — residual is None.
    let pred = Predicate::And(
        Box::new(Predicate::Cmp(
            ColRef::col("label"),
            CmpOp::Eq,
            Literal::Text("INBOX".to_string()),
        )),
        Box::new(Predicate::Cmp(
            ColRef::col("is_unread"),
            CmpOp::Eq,
            Literal::Bool(false),
        )),
    );
    let res = query::build_query(None, Some(&pred));
    assert_eq!(res.query, "label:INBOX is:read");
    assert!(
        res.residual.is_none(),
        "exact label/is_unread mappings leave no residual to re-check"
    );
}

#[test]
fn lossy_predicate_returns_residual_so_engine_refilters() {
    use qfs_types::{CmpOp, ColRef, Literal, Pattern, Predicate};

    // A lossy `from = 'x@y'` pushes the loose `from:x@y` pre-filter but MUST return the exact
    // predicate as residual, because Gmail `from:x@y` also matches e.g. "bob@x@y-domain" / a
    // `From` header of `"Alice <x@y>"` that is not equal to 'x@y' under SQL `=`. Without the
    // residual the engine would emit those over-fetched, non-exact rows — the t20 defect.
    let from_eq = Predicate::Cmp(
        ColRef::col("from"),
        CmpOp::Eq,
        Literal::Text("x@y".to_string()),
    );
    let res = query::build_query(None, Some(&from_eq));
    assert_eq!(
        res.query, "from:x@y",
        "the loose q= pre-filter is still pushed"
    );
    assert_eq!(
        res.residual,
        Some(from_eq),
        "the exact from = 'x@y' is kept as residual so the engine re-filters substring over-fetch"
    );

    // A lossy `subject = 'hello'` likewise — `subject:hello` matches "hello world".
    let subject_eq = Predicate::Cmp(
        ColRef::col("subject"),
        CmpOp::Eq,
        Literal::Text("hello".to_string()),
    );
    let res = query::build_query(None, Some(&subject_eq));
    assert_eq!(res.query, "subject:hello");
    assert_eq!(
        res.residual,
        Some(subject_eq),
        "subject = 'hello' kept as residual; subject:hello also matches 'hello world'"
    );

    // `LIKE` and the `date` bounds are lossy too: pushed as a pre-filter, kept as residual.
    let like = Predicate::Like(ColRef::col("subject"), Pattern("weekly".to_string()));
    let res = query::build_query(None, Some(&like));
    assert_eq!(res.query, "subject:weekly");
    assert_eq!(
        res.residual,
        Some(like),
        "LIKE has no Gmail operator — kept residual"
    );

    let date_gt = Predicate::Cmp(
        ColRef::col("date"),
        CmpOp::Gt,
        Literal::Int(1_700_000_500_000),
    );
    let res = query::build_query(None, Some(&date_gt));
    assert_eq!(res.query, "after:1700000500", "ms→s truncated bound");
    assert_eq!(
        res.residual,
        Some(date_gt),
        "date bound is date-granular/truncated — kept residual for the exact ms comparison"
    );
}

#[test]
fn date_string_and_between_push_after_before_never_silently_dropped() {
    use qfs_types::{CmpOp, ColRef, Literal, Predicate};

    // A `date` STRING literal (`'2024-01-01'`) is coerced to epoch-ms and pushed as `before:`.
    // Previously it hit the `_ => None` arm and the whole predicate silently vanished (time-range
    // search returned the newest N rows unfiltered — the primary bug). The residual carries the
    // coerced `Int` so the engine's local re-check orders `Timestamp` against `Int`, not against
    // the raw date string it cannot compare.
    let date_lt = Predicate::Cmp(
        ColRef::col("date"),
        CmpOp::Lt,
        Literal::Text("2024-01-01".into()),
    );
    let res = query::build_query(None, Some(&date_lt));
    assert_eq!(
        res.query, "before:1704067200",
        "date string coerced then pushed"
    );
    assert_eq!(
        res.residual,
        Some(Predicate::Cmp(
            ColRef::col("date"),
            CmpOp::Lt,
            Literal::Int(1_704_067_200_000)
        )),
        "residual coerced to epoch-ms so the local re-check can order it"
    );

    // `date BETWEEN 'a' AND 'b'` pushes BOTH bounds (previously BETWEEN pushed nothing at all).
    let between = Predicate::Between(
        ColRef::col("date"),
        Literal::Text("2023-01-01".into()),
        Literal::Text("2024-12-31".into()),
    );
    let res = query::build_query(None, Some(&between));
    assert_eq!(res.query, "after:1672531200 before:1735603200");
    assert_eq!(
        res.residual,
        Some(Predicate::Between(
            ColRef::col("date"),
            Literal::Int(1_672_531_200_000),
            Literal::Int(1_735_603_200_000),
        )),
        "coerced BETWEEN kept as residual for the exact local bounds"
    );

    // An UNPARSEABLE date string pushes nothing and stays wholly residual — never a silent no-op,
    // never a bogus bound.
    let junk = Predicate::Cmp(
        ColRef::col("date"),
        CmpOp::Lt,
        Literal::Text("last tuesday".into()),
    );
    let res = query::build_query(None, Some(&junk));
    assert_eq!(res.query, "", "an unparseable date pushes no term");
    assert_eq!(res.residual, Some(junk), "and stays residual, not dropped");
}

// ---- search pushdown reaches the client as q= --------------------------------------------

#[test]
fn search_pushes_q_and_decodes_message_rows() {
    let (_d, mock) = driver_with_mock();
    let client: &dyn GmailClient = &*mock;
    // The driver's SELECT path: a search with q= then a per-id detail fetch (the N+1 leaves).
    mock.search_ids_seed("from:alice@example.com", vec!["m1".to_string()]);
    let page = client
        .search_message_ids("from:alice@example.com", Some(5))
        .unwrap();
    assert_eq!(page.ids, vec!["m1".to_string()]);
    let msg = client.get_message("m1").unwrap();
    let row = msg.to_row();
    // Decoded into typed row values.
    assert_eq!(row.values[0], Value::Text("m1".to_string()));
    assert_eq!(row.values[3], Value::Text("alice@example.com".to_string()));
    // The mock recorded the exact q= the driver pushed + the N+1 detail leaf.
    let calls = mock.recorded();
    assert!(calls.contains(&RecordedCall::Search {
        query: "from:alice@example.com".to_string(),
        max_results: Some(5),
    }));
    assert!(calls.contains(&RecordedCall::GetMessage {
        id: "m1".to_string()
    }));
}

// ---- effect decode: drafts / labels / trash / send --------------------------------------

#[test]
fn insert_into_drafts_decodes_to_create_draft() {
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/mail/drafts")).with_args(
        draft_args(&[
            (TO_COL, Value::Text("bob@example.com".into())),
            (SUBJECT_COL, Value::Text("hi".into())),
            (BODY_COL, Value::Text("body".into())),
        ]),
    );
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::CreateDraft { draft } => {
            assert_eq!(draft.to, vec!["bob@example.com".to_string()]);
            assert_eq!(draft.subject, "hi");
        }
        other => panic!("expected CreateDraft, got {other:?}"),
    }
}

#[test]
fn positional_draft_insert_is_rejected_at_plan_time_naming_the_named_form() {
    let (d, _) = driver_with_mock();
    // A positional `insert into /mail/drafts values ('a','b','c')` has its columns filled from the
    // message READ schema (id, thread_id, date, …) — none of which is `to` — so it would silently
    // drop recipients at COMMIT while the PREVIEW already claimed one effect. The driver rejects it
    // at PLAN time so preview and apply agree, naming the named-columns form.
    let positional = draft_args(&[
        ("id", Value::Text("bob@example.com".into())),
        ("thread_id", Value::Text("hi".into())),
        ("date", Value::Text("body".into())),
    ]);
    let rejected = d
        .plan_write(&Path::new("/mail/drafts"), Verb::Insert, &positional, None)
        .expect("a to-less drafts write is rejected, not passed through")
        .unwrap_err();
    assert_eq!(rejected.code(), "invalid_path");

    // A well-formed NAMED write (it names `to`) takes the generic by-name lowering — plan_write
    // declines so nothing changes for the working form.
    let named = draft_args(&[
        (TO_COL, Value::Text("bob@example.com".into())),
        (SUBJECT_COL, Value::Text("hi".into())),
        (BODY_COL, Value::Text("body".into())),
    ]);
    assert!(d
        .plan_write(&Path::new("/mail/drafts"), Verb::Insert, &named, None)
        .is_none());
}

#[test]
fn insert_into_drafts_decodes_an_attachments_array_struct_column() {
    // The `attachments` column is the `Array(Struct{filename, mime, bytes})` shape a t92
    // `[ { filename: '…', mime: '…', bytes: X'…' } ]` literal lowers to; the effect decoder must
    // surface it as a `MailDraft` attachment carrying the raw bytes (here `X'68656c6c6f'` = "hello").
    let attachment = Value::Struct(qfs_types::Fields::new(vec![
        ("filename".to_string(), Value::Text("note.txt".into())),
        ("mime".to_string(), Value::Text("text/plain".into())),
        ("bytes".to_string(), Value::Bytes(b"hello".to_vec())),
    ]));
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/mail/drafts")).with_args(
        draft_args(&[
            (TO_COL, Value::Text("bob@example.com".into())),
            (SUBJECT_COL, Value::Text("hi".into())),
            (BODY_COL, Value::Text("body".into())),
            ("attachments", Value::Array(vec![attachment])),
        ]),
    );
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::CreateDraft { draft } => {
            assert_eq!(draft.attachments.len(), 1);
            assert_eq!(draft.attachments[0].filename, "note.txt");
            assert_eq!(draft.attachments[0].mime, "text/plain");
            assert_eq!(draft.attachments[0].bytes, b"hello");
        }
        other => panic!("expected CreateDraft, got {other:?}"),
    }
}

#[test]
fn insert_into_labels_decodes_to_create_label() {
    // gmail-ftp `mkdir Work/Receipts` → INSERT INTO /mail/labels VALUES ('Work/Receipts'). The
    // positional `name` column is named by the label collection's describe schema.
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/mail/labels")).with_args(
        draft_args(&[(NAME_COL, Value::Text("Work/Receipts".into()))]),
    );
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::CreateLabel { name } => assert_eq!(name, "Work/Receipts"),
        other => panic!("expected CreateLabel, got {other:?}"),
    }
}

#[test]
fn insert_into_labels_with_no_name_is_malformed() {
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/mail/labels"))
        .with_args(draft_args(&[("other", Value::Text("x".into()))]));
    assert!(GmailEffect::from_node(&node).is_err());
}

#[test]
fn label_create_capability_is_insert_only() {
    let (d, _) = driver_with_mock();
    let labels = Path::new("/mail/labels");
    assert!(check_capability(&d, &labels, Verb::Insert).is_ok());
    assert!(check_capability(&d, &labels, Verb::Select).is_err());
}

#[test]
fn upsert_into_drafts_is_retry_safe_keyed_by_draft_id() {
    let node = EffectNode::new(NodeId(0), EffectKind::Upsert, target("/mail/drafts")).with_args(
        draft_args(&[
            (DRAFT_ID_COL, Value::Text("d9".into())),
            (TO_COL, Value::Text("bob@example.com".into())),
        ]),
    );
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::UpsertDraft { id, .. } => assert_eq!(id, "d9"),
        other => panic!("expected UpsertDraft, got {other:?}"),
    }
}

#[test]
fn update_on_label_decodes_to_modify_labels() {
    // `update /mail/INBOX set add_labels = … where id == 'm1'`: the SET payload is `args`, the
    // exact-id match rides the WHERE-SELECTOR (§7) — the one channel a filter travels on.
    let node = EffectNode::new(NodeId(0), EffectKind::Update, target("/mail/INBOX"))
        .with_args(draft_args(&[
            (ADD_LABELS_COL, Value::Text("STARRED".into())),
            (REMOVE_LABELS_COL, Value::Text("UNREAD".into())),
        ]))
        .with_selector(draft_args(&[("id", Value::Text("m1".into()))]));
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::ModifyLabels {
            message,
            add,
            remove,
        } => {
            assert_eq!(message, "m1");
            assert_eq!(add, vec!["STARRED".to_string()]);
            assert_eq!(remove, vec!["UNREAD".to_string()]);
        }
        other => panic!("expected ModifyLabels, got {other:?}"),
    }
}

#[test]
fn remove_message_is_a_single_trash_message_not_thread() {
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("id:m1"));
    assert!(node.irreversible, "REMOVE is inherently irreversible");
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::TrashMessage { id } => assert_eq!(id, "m1"),
        other => panic!("expected TrashMessage, got {other:?}"),
    }
    // A thread address trashes the whole thread.
    let thread = EffectNode::new(NodeId(1), EffectKind::Remove, target("id:thread:t1"));
    match GmailEffect::from_node(&thread).unwrap() {
        GmailEffect::TrashThread { id } => assert_eq!(id, "t1"),
        other => panic!("expected TrashThread, got {other:?}"),
    }
}

#[test]
fn remove_on_label_with_exact_id_key_trashes_that_message() {
    // `remove /mail/INBOX where id == 'm1'` — the exact-key collection trash resolves the one named
    // message (parity with the UPDATE exact-key form), so the collection's advertised REMOVE is
    // honest and preview/apply agree for the committable shape (ticket 20260704155500).
    // The exact-id key rides the WHERE-SELECTOR (§7); a REMOVE writes nothing, so `args` is empty.
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/mail/INBOX"))
        .with_selector(draft_args(&[("id", Value::Text("m1".into()))]));
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::TrashMessage { id } => assert_eq!(id, "m1"),
        other => panic!("expected TrashMessage, got {other:?}"),
    }
}

#[test]
fn set_wide_collection_writes_fail_closed_with_a_clear_reason() {
    use crate::error::GmailError;
    // A collection REMOVE / UPDATE with NO exact `id` key (a set-wide predicate reached the applier
    // as no key) is refused CLOSED with a MalformedEffect naming the committable form — never a
    // silent over-match of Gmail's lossy search, and never a CapabilityDenied that would contradict
    // describe's REMOVE/UPDATE claim on the collection.
    let rm = EffectNode::new(NodeId(0), EffectKind::Remove, target("/mail/INBOX"));
    match GmailEffect::from_node(&rm) {
        Err(GmailError::MalformedEffect { verb, reason, .. }) => {
            assert_eq!(verb, "REMOVE");
            assert!(reason.contains("where id =="), "names the form: {reason}");
        }
        other => panic!("expected MalformedEffect naming the id form, got {other:?}"),
    }
    let up = EffectNode::new(NodeId(1), EffectKind::Update, target("/mail/INBOX")).with_args(
        draft_args(&[(ADD_LABELS_COL, Value::Text("STARRED".into()))]),
    );
    match GmailEffect::from_node(&up) {
        Err(GmailError::MalformedEffect { verb, reason, .. }) => {
            assert_eq!(verb, "UPDATE");
            assert!(reason.contains("where id =="), "names the form: {reason}");
        }
        other => panic!("expected MalformedEffect naming the id form, got {other:?}"),
    }
}

#[test]
fn update_on_a_message_node_relabels_the_single_message() {
    // A message node (`/mail/<label>/<msg>`) is Update-capable: relabel the one message with no key
    // ambiguity — the committable relabel form the cookbook teaches.
    let node = EffectNode::new(NodeId(0), EffectKind::Update, target("/mail/INBOX/m9")).with_args(
        draft_args(&[(ADD_LABELS_COL, Value::Text("STARRED".into()))]),
    );
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::ModifyLabels { message, add, .. } => {
            assert_eq!(message, "m9");
            assert_eq!(add, vec!["STARRED".to_string()]);
        }
        other => panic!("expected ModifyLabels, got {other:?}"),
    }
    // describe/caps honesty: the message node advertises Update (the applier services it).
    let (driver, _) = driver_with_mock();
    let msg = Path::new("/mail/INBOX/m9");
    assert!(check_capability(&driver, &msg, Verb::Update).is_ok());
}

#[test]
fn call_mail_send_decodes_irreversible_and_unknown_proc_rejected() {
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("mail.send")),
        target("/mail/drafts"),
    )
    .irreversible(true)
    .with_args(draft_args(&[(DRAFT_ID_COL, Value::Text("d9".into()))]));
    assert!(node.irreversible);
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::Send { draft_id, .. } => assert_eq!(draft_id, Some("d9".to_string())),
        other => panic!("expected Send, got {other:?}"),
    }
    // An undeclared CALL is rejected structurally.
    let bad = EffectNode::new(
        NodeId(1),
        EffectKind::Call(ProcId::new("mail.nuke")),
        target("/mail/drafts"),
    );
    assert_eq!(
        GmailEffect::from_node(&bad).unwrap_err().code(),
        "unknown_procedure"
    );
}

// ---- MIME golden -------------------------------------------------------------------------

#[test]
fn build_mime_is_byte_stable_for_nonascii_subject_and_two_attachments() {
    let draft = MailDraft {
        id: None,
        to: vec!["bob@example.com".to_string()],
        cc: vec![],
        subject: "café ☕".to_string(), // non-ASCII → RFC 2047 base64
        body: "hello\nworld".to_string(),
        attachments: vec![
            Attachment {
                filename: "a.txt".to_string(),
                mime: "text/plain".to_string(),
                bytes: b"AAAA".to_vec(),
            },
            Attachment {
                filename: "b.bin".to_string(),
                mime: "application/octet-stream".to_string(),
                bytes: vec![0u8, 1, 2, 3],
            },
        ],
        reply: None,
    };
    let bytes = mime::build_mime(&draft).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    // CRLF line endings, RFC 2047 subject, multipart/mixed boundary, base64 parts.
    assert!(text.contains("\r\n"));
    assert!(
        text.contains("Subject: =?UTF-8?B?"),
        "non-ascii subject is RFC2047-encoded"
    );
    assert!(text.contains("Content-Type: multipart/mixed; boundary="));
    assert!(text.contains("Content-Disposition: attachment; filename=\"a.txt\""));
    assert!(text.contains("QUFBQQ=="), "AAAA base64 part present");
    // Byte-stable: same input → identical output (deterministic boundary, no randomness).
    assert_eq!(mime::build_mime(&draft).unwrap(), text.into_bytes());

    // The base64url raw is the Gmail `raw` field shape (URL alphabet, no + or /).
    let raw = mime::raw_base64url(&draft).unwrap();
    assert!(!raw.contains('+') && !raw.contains('/'), "raw is base64url");
}

#[test]
fn build_mime_rejects_a_draft_with_no_recipients() {
    let draft = MailDraft {
        to: vec![],
        ..MailDraft::default()
    };
    assert_eq!(mime::build_mime(&draft).unwrap_err().code(), "mime");
}

// ---- PREVIEW performs no I/O (mock asserts zero calls) -----------------------------------

#[test]
fn preview_of_a_send_plan_performs_no_io() {
    let (_d, mock) = driver_with_mock();
    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.send")),
            target("/mail/drafts"),
        )
        .irreversible(true)
        .with_args(draft_args(&[(DRAFT_ID_COL, Value::Text("d9".into()))])),
    );
    let plan = b.build();
    // The pure preview walks the plan with NO client dispatch.
    let pv = preview(&plan);
    assert_eq!(pv.rows.len(), 1);
    assert!(
        pv.rows[0].irreversible,
        "preview surfaces the irreversible send"
    );
    assert!(
        mock.recorded().is_empty(),
        "PREVIEW must perform zero Gmail API calls: {:?}",
        mock.recorded()
    );
}

// ---- token never in logs / errors --------------------------------------------------------

#[test]
fn errors_are_secret_free() {
    // Every GmailError surface is secret-free by construction; assert no token-looking text.
    let errs = [
        GmailError::Api {
            op: "messages.send",
            status: 500,
        },
        GmailError::CapabilityDenied {
            path: "/mail/drafts".into(),
            verb: "UPDATE",
        },
        GmailError::from(qfs_google_auth::AuthError::TokenRefresh {
            reason: "invalid_grant".to_string(),
        }),
    ];
    for e in &errs {
        let text = format!("{e} {e:?}");
        assert!(!text.contains("Bearer"), "no bearer in error: {text}");
        assert!(!text.contains("ya29"), "no google token prefix: {text}");
    }
    // The auth→gmail mapping preserves only the secret-free code + reauthorize signal.
    match GmailError::from(qfs_google_auth::AuthError::TokenRefresh {
        reason: "invalid_grant".to_string(),
    }) {
        GmailError::Auth { code, reauthorize } => {
            assert_eq!(code, "auth_token_refresh");
            assert!(reauthorize);
        }
        other => panic!("expected Auth, got {other:?}"),
    }
}

// ---- end-to-end: commit through interpreter + bridge -------------------------------------

#[tokio::test]
async fn commit_trash_message_end_to_end_through_interpreter() {
    let (driver, mock) = driver_with_mock();
    let bridge = gmail_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(EffectNode::new(
        NodeId(0),
        EffectKind::Remove,
        target("id:m1"),
    ));
    let plan = b.build();
    plan.validate().unwrap();

    let caps = CapabilitySet::none().grant(DriverId::new("mail"), &EffectKind::Remove);
    let outcome = interp.commit(plan, &caps).await.unwrap();

    assert!(outcome.is_complete(), "trash applied: {outcome:?}");
    assert_eq!(outcome.applied_ids(), vec![NodeId(0)]);
    // The applier dispatched exactly one trash-message op (the irreversible REMOVE).
    assert_eq!(
        mock.recorded(),
        vec![RecordedCall::TrashMessage {
            id: "m1".to_string()
        }]
    );
}

#[tokio::test]
async fn commit_send_creates_draft_then_sends_it() {
    let (driver, mock) = driver_with_mock();
    let bridge = gmail_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    // mail.send with draft content → create-then-send (the recoverable de-dupe path).
    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.send")),
            target("/mail/drafts"),
        )
        .irreversible(true)
        .with_args(draft_args(&[
            (TO_COL, Value::Text("bob@example.com".into())),
            (SUBJECT_COL, Value::Text("hi".into())),
            (BODY_COL, Value::Text("body".into())),
        ])),
    );
    let plan = b.build();

    let caps = CapabilitySet::none().grant(
        DriverId::new("mail"),
        &EffectKind::Call(ProcId::new("mail.send")),
    );
    let outcome = interp.commit(plan, &caps).await.unwrap();
    assert!(outcome.is_complete(), "send applied: {outcome:?}");

    // create_draft then send_draft, in order — and NEVER a permanent delete.
    let calls = mock.recorded();
    assert!(matches!(
        calls.first(),
        Some(RecordedCall::CreateDraft { .. })
    ));
    assert!(matches!(calls.get(1), Some(RecordedCall::SendDraft { .. })));
}

#[test]
fn addressed_draft_path_decodes_to_send_by_that_draft_id() {
    // The regression fix: `/mail/drafts/<id> |> call mail.send` carries the draft id in the effect
    // node's TARGET PATH (a CALL's args are its literal arguments, never upstream rows — the only
    // channel for a per-draft id is the path). `decode_call` must lower that path segment into
    // `Send { draft_id }`; before, it read only a `draft_id` COLUMN that no query produces, so every
    // form fell into a byteless create-then-send and failed "draft has no `to`".
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("mail.send")),
        target("/mail/drafts/d42"),
    )
    .irreversible(true);
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::Send { draft_id, draft } => {
            assert_eq!(draft_id, Some("d42".to_string()));
            assert!(
                draft.is_none(),
                "an addressed send carries no create-draft body"
            );
        }
        other => panic!("expected Send by id, got {other:?}"),
    }
    // Path parse parity: `/mail/drafts/<id>` is a DRAFT node, not a message under a `drafts` label.
    assert_eq!(
        MailPath::parse_str("/mail/drafts/d42").unwrap(),
        MailPath::Draft {
            id: "d42".to_string()
        }
    );
}

#[tokio::test]
async fn commit_send_of_an_addressed_draft_sends_by_id_without_recreating() {
    // End-to-end through the interpreter: sending an addressed existing draft dispatches ONLY
    // `drafts.send` for that draft id — no `drafts.create` (contrast `commit_send_creates_draft_…`,
    // the content-carrying create-then-send). This is the form the cookbook now teaches.
    let (driver, mock) = driver_with_mock();
    let bridge = gmail_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.send")),
            target("/mail/drafts/d42"),
        )
        .irreversible(true),
    );
    let caps = CapabilitySet::none().grant(
        DriverId::new("mail"),
        &EffectKind::Call(ProcId::new("mail.send")),
    );
    let outcome = interp.commit(b.build(), &caps).await.unwrap();
    assert!(outcome.is_complete(), "send applied: {outcome:?}");

    let calls = mock.recorded();
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, RecordedCall::CreateDraft { .. })),
        "an existing-draft send must NOT create a draft: {calls:?}"
    );
    assert!(
        matches!(calls.as_slice(), [RecordedCall::SendDraft { draft_id }] if draft_id == "d42"),
        "exactly one send of the addressed draft id: {calls:?}"
    );
}

#[test]
fn plan_call_rejects_a_byteless_send_and_accepts_resolvable_forms() {
    // Plan-time guard (preview == apply): a `mail.send` that resolves NO draft/recipient — the bare
    // `/mail/drafts |> call mail.send` collection form with no args — is rejected HERE, at plan
    // time, instead of at COMMIT with a confusing malformed-INSERT. An addressed draft, a `draft_id`
    // arg, or a non-empty `to` all resolve and pass.
    let (d, _) = driver_with_mock();
    let empty = RowBatch::new(Schema::new(vec![]), vec![Row::new(vec![])]);

    // Byteless collection send → rejected with an actionable path error.
    let refused = d
        .plan_call(&Path::new("/mail/drafts"), "mail.send", &empty)
        .expect("mail.send is guarded")
        .unwrap_err();
    assert_eq!(refused.code(), "invalid_path");

    // Addressed draft → allowed (args empty; the path resolves the draft).
    assert!(d
        .plan_call(&Path::new("/mail/drafts/d1"), "mail.send", &empty)
        .expect("guarded")
        .is_ok());

    // Recipient-bearing create-then-send → allowed.
    let with_to = draft_args(&[(TO_COL, Value::Text("x@example.com".into()))]);
    assert!(d
        .plan_call(&Path::new("/mail/drafts"), "mail.send", &with_to)
        .expect("guarded")
        .is_ok());

    // An empty `to` does NOT resolve — rejected like the byteless form.
    let empty_to = draft_args(&[(TO_COL, Value::Text(String::new()))]);
    assert!(d
        .plan_call(&Path::new("/mail/drafts"), "mail.send", &empty_to)
        .expect("guarded")
        .is_err());

    // A non-`mail.send` CALL is not this driver's concern (declines).
    assert!(d
        .plan_call(&Path::new("/mail/drafts"), "mail.other", &empty)
        .is_none());
}

// ---- mail.reply: thread-reply draft ------------------------------------------------------

#[test]
fn mail_reply_is_declared_reversible_with_compose_scope() {
    // A reply CREATES A DRAFT — reversible (unlike the irreversible mail.send). DESCRIBE reports it
    // honestly: the proc is declared, reversible, and needs the compose scope to draft.
    let (d, _) = driver_with_mock();
    let reply = resolve_proc(&d, "reply").unwrap();
    assert!(
        !reply.irreversible,
        "mail.reply drafts (reversible), never sends"
    );
    assert_eq!(reply.requires_scopes, vec![GMAIL_COMPOSE_SCOPE.to_string()]);
    // body is the first (required) param; to/cc/subject are optional overrides.
    assert_eq!(reply.params.first().map(|p| p.name.as_str()), Some("body"));
}

#[test]
fn call_mail_reply_decodes_to_a_reversible_reply_at_the_parent_node() {
    // `id:<msg> |> call mail.reply(body => …)` decodes to a Reply addressed at the parent message.
    // `to`/`subject` are NOT resolved here — they default against the parent at COMMIT — so an
    // omitted `to` stays empty and `subject` stays None until apply.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("mail.reply")),
        target("id:m1"),
    )
    .with_args(draft_args(&[(BODY_COL, Value::Text("thanks!".into()))]));
    let effect = GmailEffect::from_node(&node).unwrap();
    assert!(
        !effect.is_irreversible(),
        "a reply drafts — it is reversible, not a send/trash"
    );
    match effect {
        GmailEffect::Reply {
            parent,
            to,
            cc,
            subject,
            body,
            attachments,
        } => {
            assert_eq!(parent, "m1");
            assert_eq!(body, "thanks!");
            assert!(to.is_empty(), "no `to` override → defaulted at apply");
            assert!(cc.is_empty());
            assert!(
                subject.is_none(),
                "no `subject` override → defaulted at apply"
            );
            assert!(attachments.is_empty(), "no attachments on this reply");
        }
        other => panic!("expected Reply, got {other:?}"),
    }

    // The `/mail/<label>/<msg>` path form is the same parent node.
    let path_node = EffectNode::new(
        NodeId(1),
        EffectKind::Call(ProcId::new("mail.reply")),
        target("/mail/INBOX/m1"),
    )
    .with_args(draft_args(&[(BODY_COL, Value::Text("ok".into()))]));
    assert!(matches!(
        GmailEffect::from_node(&path_node).unwrap(),
        GmailEffect::Reply { parent, .. } if parent == "m1"
    ));
}

#[test]
fn mail_reply_without_a_body_or_off_a_non_message_is_malformed() {
    use crate::error::GmailError;
    // No body → malformed (the applier would have nothing to say).
    let no_body = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("mail.reply")),
        target("id:m1"),
    );
    assert!(matches!(
        GmailEffect::from_node(&no_body),
        Err(GmailError::MalformedEffect { verb: "CALL", .. })
    ));
    // Addressed at the drafts collection (not a message) → malformed, names the parent-message form.
    let not_a_message = EffectNode::new(
        NodeId(1),
        EffectKind::Call(ProcId::new("mail.reply")),
        target("/mail/drafts"),
    )
    .with_args(draft_args(&[(BODY_COL, Value::Text("hi".into()))]));
    match GmailEffect::from_node(&not_a_message) {
        Err(GmailError::MalformedEffect { reason, .. }) => {
            assert!(
                reason.contains("parent message"),
                "names the form: {reason}"
            );
        }
        other => panic!("expected MalformedEffect, got {other:?}"),
    }
}

#[test]
fn build_mime_of_a_reply_carries_in_reply_to_and_references() {
    // The reply draft's MIME must carry In-Reply-To + References pointing at the parent Message-Id
    // so EVERY mail client threads it (not only Gmail's server-side threadId). Both headers precede
    // the Content-Type block, so a plain-text reply (no attachments) inherits them.
    let draft = MailDraft {
        to: vec!["alice@example.com".to_string()],
        subject: "Re: hello".to_string(),
        body: "thanks!".to_string(),
        reply: Some(ReplyContext {
            thread_id: "t1".to_string(),
            references: "<parent-msgid@mail.gmail.com>".to_string(),
        }),
        ..MailDraft::default()
    };
    let text = String::from_utf8(mime::build_mime(&draft).unwrap()).unwrap();
    assert!(
        text.contains("In-Reply-To: <parent-msgid@mail.gmail.com>\r\n"),
        "In-Reply-To header present: {text}"
    );
    assert!(
        text.contains("References: <parent-msgid@mail.gmail.com>\r\n"),
        "References header present: {text}"
    );
    // A standalone draft (reply: None) carries neither header.
    let standalone = MailDraft {
        to: vec!["a@b".to_string()],
        ..MailDraft::default()
    };
    let plain = String::from_utf8(mime::build_mime(&standalone).unwrap()).unwrap();
    assert!(
        !plain.contains("In-Reply-To"),
        "no threading header on a standalone draft"
    );
}

#[tokio::test]
async fn commit_reply_creates_a_threaded_draft_with_defaulted_to_and_subject() {
    // End-to-end: `id:m1 |> call mail.reply(body => …)` reads the parent (m1: thread t1, From
    // alice, Subject hello, Message-Id <parent-msgid…>) and creates ONE draft carrying the parent's
    // threadId (server-side threading) and In-Reply-To/References (client-side), with `to` defaulted
    // to the parent's From and `subject` to `Re: hello`. NEVER a send — a reply is reversible.
    let (driver, mock) = driver_with_mock();
    let bridge = gmail_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.reply")),
            target("id:m1"),
        )
        .with_args(draft_args(&[(
            BODY_COL,
            Value::Text("thanks, will do".into()),
        )])),
    );
    let caps = CapabilitySet::none().grant(
        DriverId::new("mail"),
        &EffectKind::Call(ProcId::new("mail.reply")),
    );
    let outcome = interp.commit(b.build(), &caps).await.unwrap();
    assert!(outcome.is_complete(), "reply drafted: {outcome:?}");

    // The parent was read, then exactly one draft created carrying the parent's thread id — and NO
    // send (a reply is a reversible draft, sent later by addressing /mail/drafts/<id>).
    let calls = mock.recorded();
    assert!(
        calls.contains(&RecordedCall::GetMessage {
            id: "m1".to_string()
        }),
        "parent read to resolve the thread: {calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, RecordedCall::SendDraft { .. })),
        "a reply drafts, never sends: {calls:?}"
    );
    let raw = match calls.iter().find_map(|c| match c {
        RecordedCall::CreateDraft { raw, thread_id } => Some((raw.clone(), thread_id.clone())),
        _ => None,
    }) {
        Some(v) => v,
        None => panic!("expected a CreateDraft: {calls:?}"),
    };
    assert_eq!(
        raw.1,
        Some("t1".to_string()),
        "the create carries the parent's threadId (server-side threading)"
    );
    let mime = String::from_utf8(crate::mime::decode_base64url(&raw.0).unwrap()).unwrap();
    assert!(
        mime.contains("To: alice@example.com\r\n"),
        "`to` defaults to the parent From: {mime}"
    );
    assert!(
        mime.contains("Subject: Re: hello\r\n"),
        "`subject` defaults to Re: <parent subject>: {mime}"
    );
    assert!(
        mime.contains("In-Reply-To: <parent-msgid@mail.gmail.com>\r\n")
            && mime.contains("References: <parent-msgid@mail.gmail.com>\r\n"),
        "client-side threading headers reference the parent Message-Id: {mime}"
    );
    assert!(
        mime.contains("thanks, will do"),
        "the body is carried: {mime}"
    );
}

#[tokio::test]
async fn reply_draft_then_send_preserves_the_thread() {
    // Capability 5 falls out of capability 3 + the existing send: after `mail.reply` drafts into the
    // thread (mock returns draft id `draft-new`), sending it is the SHIPPED `/mail/drafts/<id> |>
    // call mail.send` — one drafts.send of THAT id, no re-create. The thread is preserved because
    // the draft was created carrying threadId; drafts.send keeps it. No second threaded-send path.
    let (driver, mock) = driver_with_mock();
    let bridge = gmail_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    // 1) Reply → threaded draft `draft-new`.
    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.reply")),
            target("id:m1"),
        )
        .with_args(draft_args(&[(BODY_COL, Value::Text("ack".into()))])),
    );
    let reply_caps = CapabilitySet::none().grant(
        DriverId::new("mail"),
        &EffectKind::Call(ProcId::new("mail.reply")),
    );
    interp.commit(b.build(), &reply_caps).await.unwrap();

    // 2) Send THAT draft by addressing it — the existing send-by-id path.
    let mut b2 = PlanBuilder::new();
    b2.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.send")),
            target("/mail/drafts/draft-new"),
        )
        .irreversible(true),
    );
    let send_caps = CapabilitySet::none().grant(
        DriverId::new("mail"),
        &EffectKind::Call(ProcId::new("mail.send")),
    );
    interp.commit(b2.build(), &send_caps).await.unwrap();

    let calls = mock.recorded();
    // The send re-sent the reply draft by id — no fresh create in the send step.
    assert!(
        matches!(
            calls.last(),
            Some(RecordedCall::SendDraft { draft_id }) if draft_id == "draft-new"
        ),
        "the reply draft is sent by its id (thread preserved by the draft's threadId): {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|c| matches!(c, RecordedCall::CreateDraft { .. }))
            .count(),
        1,
        "exactly one create (the reply) — the send re-uses the draft, never re-creates: {calls:?}"
    );
}

#[test]
fn reply_to_a_message_with_no_message_id_fails_closed() {
    use qfs_runtime::SharedApplier;
    // A parent with no resolvable Message-Id cannot be threaded client-side. The applier fails
    // CLOSED with an actionable, secret-free error (no panic, no bare header) rather than drafting a
    // reply that no mail client threads.
    let mock = Arc::new(MockGmailClient::new().with_message(MailMessage {
        id: "m2".to_string(),
        thread_id: "t2".to_string(),
        label_ids: vec![],
        date: 0,
        from: "bob@example.com".to_string(),
        subject: "no msgid".to_string(),
        snippet: String::new(),
        message_id: String::new(), // Gmail returned no Message-Id header
        attachments: vec![],
    }));
    let driver = GmailDriver::new(mock.clone() as Arc<dyn GmailClient>);
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("mail.reply")),
        target("id:m2"),
    )
    .with_args(draft_args(&[(BODY_COL, Value::Text("hi".into()))]));
    let err = driver.gmail_applier().apply_shared(&node).unwrap_err();
    let text = format!("{err:?}");
    assert!(
        text.contains("thread") || text.contains("Message-Id"),
        "actionable: {text}"
    );
    assert!(
        !text.contains("Bearer") && !text.contains("ya29"),
        "secret-free: {text}"
    );
    // Nothing was drafted (fail closed before create).
    assert!(
        !mock
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::CreateDraft { .. })),
        "no draft created when the parent cannot be threaded"
    );
}

#[test]
fn plan_call_guards_mail_reply_to_an_addressed_parent_with_a_body() {
    // Plan-time guard (preview == apply): mail.reply needs a parent MESSAGE node + a `body`. An
    // addressed message (id: or path form) with a body resolves; a bodyless call, or one addressed
    // at a non-message node, is refused HERE with the actionable reply form.
    let (d, _) = driver_with_mock();
    let with_body = draft_args(&[(BODY_COL, Value::Text("hi".into()))]);

    assert!(d
        .plan_call(&Path::new("id:m1"), "mail.reply", &with_body)
        .expect("guarded")
        .is_ok());
    assert!(d
        .plan_call(&Path::new("/mail/INBOX/m1"), "mail.reply", &with_body)
        .expect("guarded")
        .is_ok());

    // No body → refused.
    let empty = RowBatch::new(Schema::new(vec![]), vec![Row::new(vec![])]);
    assert_eq!(
        d.plan_call(&Path::new("id:m1"), "mail.reply", &empty)
            .expect("guarded")
            .unwrap_err()
            .code(),
        "invalid_path"
    );
    // Addressed at the drafts collection (no parent) → refused even with a body.
    assert!(d
        .plan_call(&Path::new("/mail/drafts"), "mail.reply", &with_body)
        .expect("guarded")
        .is_err());
}

#[test]
fn preview_of_a_reply_plan_performs_no_io() {
    // A reply PREVIEW decodes nothing and touches no Gmail API (the parent read happens only at
    // COMMIT, in the applier). The mock asserts zero calls.
    let (_d, mock) = driver_with_mock();
    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.reply")),
            target("id:m1"),
        )
        .with_args(draft_args(&[(BODY_COL, Value::Text("hi".into()))])),
    );
    let pv = preview(&b.build());
    assert_eq!(pv.rows.len(), 1);
    assert!(
        !pv.rows[0].irreversible,
        "a reply is reversible — PREVIEW must not flag it irreversible"
    );
    assert!(
        mock.recorded().is_empty(),
        "PREVIEW must perform zero Gmail API calls: {:?}",
        mock.recorded()
    );
}

// ---- attach / detach across every draft / send / reply form ------------------------------

/// One `{ filename, mime, bytes }` write-attachment struct — the array element an `attachments`
/// column/param carries on every form (the write shape; a read yields `{filename, mime, size}`).
fn attach(filename: &str, mime: &str, bytes: &[u8]) -> Value {
    Value::Struct(qfs_types::Fields::new(vec![
        ("filename".to_string(), Value::Text(filename.into())),
        ("mime".to_string(), Value::Text(mime.into())),
        ("bytes".to_string(), Value::Bytes(bytes.to_vec())),
    ]))
}

/// The number of attachment parts in a base64url `raw` message (one `Content-Disposition:
/// attachment` per attached file) — the hermetic proof a form carried, or dropped, an attachment.
fn attachment_parts_in_raw(raw: &str) -> usize {
    let mime = String::from_utf8(crate::mime::decode_base64url(raw).unwrap()).unwrap();
    mime.matches("Content-Disposition: attachment").count()
}

#[test]
fn insert_into_replies_decodes_to_a_reversible_reply_with_cross_service_attachment() {
    // `… |> insert into /mail/<label>/<msg>/replies` — the pipeline-composable reply. The parent id
    // rides the path (`m1`), and the materialized row carries `body` + an `attachments` array sourced
    // from another service. It decodes to the SAME reversible Reply the CALL form produces.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/mail/INBOX/m1/replies"),
    )
    .with_args(draft_args(&[
        (BODY_COL, Value::Text("See the attached report.".into())),
        (
            "attachments",
            Value::Array(vec![attach("report.pdf", "application/pdf", b"%PDF-1.7 x")]),
        ),
    ]));
    let effect = GmailEffect::from_node(&node).unwrap();
    assert!(
        !effect.is_irreversible(),
        "an INSERT-sourced reply drafts — reversible like the CALL form"
    );
    match effect {
        GmailEffect::Reply {
            parent,
            body,
            attachments,
            ..
        } => {
            assert_eq!(parent, "m1", "the parent id comes from the path segment");
            assert_eq!(body, "See the attached report.");
            assert_eq!(
                attachments.len(),
                1,
                "the cross-service attachment survived"
            );
            assert_eq!(attachments[0].filename, "report.pdf");
            assert_eq!(attachments[0].mime, "application/pdf");
            assert_eq!(attachments[0].bytes, b"%PDF-1.7 x");
        }
        other => panic!("expected Reply, got {other:?}"),
    }
}

#[test]
fn insert_into_replies_without_a_body_is_malformed() {
    use crate::error::GmailError;
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/mail/INBOX/m1/replies"),
    );
    assert!(matches!(
        GmailEffect::from_node(&node),
        Err(GmailError::MalformedEffect { verb: "INSERT", .. })
    ));
}

#[test]
fn call_mail_send_carries_attachments_on_the_created_draft() {
    // Attach on a create-then-send: `call mail.send(to, subject, body, attachments => [...])`. The
    // send proc now declares the `attachments` param, so the arg reaches the `attachments` column
    // the decoder already reads — the built draft carries the multipart attachment.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("mail.send")),
        target("/mail/drafts"),
    )
    .irreversible(true)
    .with_args(draft_args(&[
        (TO_COL, Value::Text("bob@example.com".into())),
        (SUBJECT_COL, Value::Text("hi".into())),
        (BODY_COL, Value::Text("see attached".into())),
        (
            "attachments",
            Value::Array(vec![attach("a.txt", "text/plain", b"hello")]),
        ),
    ]));
    match GmailEffect::from_node(&node).unwrap() {
        GmailEffect::Send {
            draft: Some(d),
            draft_id: None,
        } => {
            assert_eq!(d.attachments.len(), 1);
            assert_eq!(d.attachments[0].filename, "a.txt");
            let raw = mime::raw_base64url(&d).unwrap();
            assert_eq!(
                attachment_parts_in_raw(&raw),
                1,
                "the send draft carries the file"
            );
        }
        other => panic!("expected a create-then-send carrying an attachment, got {other:?}"),
    }
}

#[test]
fn insert_and_upsert_still_attach_and_upsert_full_replace_is_detach() {
    // INSERT/UPSERT already attach; assert the MIME carries the parts (regression). Then DETACH =
    // UPSERT full-replace: the UPSERT row's `attachments` array IS the draft's whole set — omit one
    // to drop it, omit all to clear. No new keyword, no per-attachment API (settled design).
    let two = EffectNode::new(NodeId(0), EffectKind::Upsert, target("/mail/drafts")).with_args(
        draft_args(&[
            (DRAFT_ID_COL, Value::Text("d1".into())),
            (TO_COL, Value::Text("bob@example.com".into())),
            (
                "attachments",
                Value::Array(vec![
                    attach("a.txt", "text/plain", b"A"),
                    attach("b.txt", "text/plain", b"B"),
                ]),
            ),
        ]),
    );
    let GmailEffect::UpsertDraft { draft, .. } = GmailEffect::from_node(&two).unwrap() else {
        panic!("expected UpsertDraft");
    };
    assert_eq!(
        attachment_parts_in_raw(&mime::raw_base64url(&draft).unwrap()),
        2,
        "both attachments present"
    );

    // Detach one: a later UPSERT (same id) naming only ONE attachment drops the other.
    let one = EffectNode::new(NodeId(1), EffectKind::Upsert, target("/mail/drafts")).with_args(
        draft_args(&[
            (DRAFT_ID_COL, Value::Text("d1".into())),
            (TO_COL, Value::Text("bob@example.com".into())),
            (
                "attachments",
                Value::Array(vec![attach("a.txt", "text/plain", b"A")]),
            ),
        ]),
    );
    let GmailEffect::UpsertDraft { draft, .. } = GmailEffect::from_node(&one).unwrap() else {
        panic!("expected UpsertDraft");
    };
    assert_eq!(
        attachment_parts_in_raw(&mime::raw_base64url(&draft).unwrap()),
        1,
        "the omitted attachment is detached"
    );

    // Detach all: an UPSERT with no `attachments` column clears them — a valid single-part message.
    let none = EffectNode::new(NodeId(2), EffectKind::Upsert, target("/mail/drafts")).with_args(
        draft_args(&[
            (DRAFT_ID_COL, Value::Text("d1".into())),
            (TO_COL, Value::Text("bob@example.com".into())),
        ]),
    );
    let GmailEffect::UpsertDraft { draft, .. } = GmailEffect::from_node(&none).unwrap() else {
        panic!("expected UpsertDraft");
    };
    let raw = mime::raw_base64url(&draft).unwrap();
    assert_eq!(attachment_parts_in_raw(&raw), 0, "all attachments detached");
    let text = String::from_utf8(crate::mime::decode_base64url(&raw).unwrap()).unwrap();
    assert!(
        text.contains("Content-Type: text/plain; charset=\"UTF-8\"")
            && !text.contains("multipart/mixed"),
        "a zero-attachment draft is a valid single-part message: {text}"
    );
}

#[test]
fn detach_then_reattach_round_trips_through_the_applier() {
    use qfs_runtime::SharedApplier;
    // Detach-then-reattach: UPSERT two → UPSERT none (detach all) → UPSERT one (reattach). Each
    // recorded raw reflects exactly the latest row's attachment set (full-replace semantics).
    let (driver, mock) = driver_with_mock();
    let upsert = |id: &str, atts: Vec<Value>| {
        EffectNode::new(NodeId(0), EffectKind::Upsert, target("/mail/drafts")).with_args(
            draft_args(&[
                (DRAFT_ID_COL, Value::Text(id.into())),
                (TO_COL, Value::Text("bob@example.com".into())),
                ("attachments", Value::Array(atts)),
            ]),
        )
    };
    driver
        .gmail_applier()
        .apply_shared(&upsert(
            "d1",
            vec![
                attach("a.txt", "text/plain", b"A"),
                attach("b.txt", "text/plain", b"B"),
            ],
        ))
        .unwrap();
    driver
        .gmail_applier()
        .apply_shared(&upsert("d1", vec![]))
        .unwrap();
    driver
        .gmail_applier()
        .apply_shared(&upsert("d1", vec![attach("c.txt", "text/plain", b"C")]))
        .unwrap();

    let parts: Vec<usize> = mock
        .recorded()
        .iter()
        .filter_map(|c| match c {
            RecordedCall::UpsertDraft { raw, .. } => Some(attachment_parts_in_raw(raw)),
            _ => None,
        })
        .collect();
    assert_eq!(
        parts,
        vec![2, 0, 1],
        "each UPSERT fully replaces the attachment set: attach 2 → detach all → reattach 1"
    );
}

#[test]
fn a_received_files_bytes_reattach_from_the_byte_read_not_the_listing() {
    // Re-attach a received file: the listing row is `{filename, mime, size}` (NO bytes); the bytes
    // come from the attachment byte-read (`/mail/<label>/<msg>/<att>` → `content` Bytes). Feeding
    // those bytes (a `Value::Bytes`, exactly what the byte-read yields) into a new draft's
    // `attachments` bytes produces an outgoing message carrying the file — the read/write shape
    // asymmetry resolved by sourcing bytes from the byte-read.
    let received: &[u8] = b"\x00\x01\x02 pretend these came from attachments.get";
    let insert = EffectNode::new(NodeId(0), EffectKind::Insert, target("/mail/drafts")).with_args(
        draft_args(&[
            (TO_COL, Value::Text("bob@example.com".into())),
            (
                "attachments",
                Value::Array(vec![attach(
                    "forwarded.bin",
                    "application/octet-stream",
                    received,
                )]),
            ),
        ]),
    );
    let GmailEffect::CreateDraft { draft } = GmailEffect::from_node(&insert).unwrap() else {
        panic!("expected CreateDraft");
    };
    assert_eq!(
        draft.attachments[0].bytes, received,
        "the received bytes flow verbatim from the byte-read into the draft attachment"
    );
    assert_eq!(
        attachment_parts_in_raw(&mime::raw_base64url(&draft).unwrap()),
        1,
        "the re-attached file rides the outgoing message"
    );
}

#[tokio::test]
async fn commit_reply_with_attachments_threads_and_carries_the_files() {
    // Attach on the REPLY form (the "every form" the sibling ticket must cover): a reply carrying
    // `attachments` creates a draft that BOTH threads (parent threadId) AND carries the file.
    let (driver, mock) = driver_with_mock();
    let bridge = gmail_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.reply")),
            target("id:m1"),
        )
        .with_args(draft_args(&[
            (BODY_COL, Value::Text("here's the file".into())),
            (
                "attachments",
                Value::Array(vec![attach("report.pdf", "application/pdf", b"%PDF-1.4")]),
            ),
        ])),
    );
    let caps = CapabilitySet::none().grant(
        DriverId::new("mail"),
        &EffectKind::Call(ProcId::new("mail.reply")),
    );
    interp.commit(b.build(), &caps).await.unwrap();

    let (raw, thread_id) = mock
        .recorded()
        .iter()
        .find_map(|c| match c {
            RecordedCall::CreateDraft { raw, thread_id } => Some((raw.clone(), thread_id.clone())),
            _ => None,
        })
        .expect("a reply create");
    assert_eq!(thread_id, Some("t1".to_string()), "the reply threads");
    assert_eq!(
        attachment_parts_in_raw(&raw),
        1,
        "the reply carries its attachment"
    );
}

#[tokio::test]
async fn multi_account_selects_independent_clients() {
    // Two accounts → two independent driver instances over two independent mock clients.
    // Each driver routes only to its own client; selection is the t19 base (one client per
    // account). We prove a label-modify on account A never touches account B's client.
    let mock_a = Arc::new(MockGmailClient::new());
    let mock_b = Arc::new(MockGmailClient::new());
    let driver_a = GmailDriver::new(mock_a.clone() as Arc<dyn GmailClient>);
    let driver_b = GmailDriver::new(mock_b.clone() as Arc<dyn GmailClient>);

    let node = EffectNode::new(NodeId(0), EffectKind::Update, target("/mail/INBOX"))
        .with_args(draft_args(&[(
            ADD_LABELS_COL,
            Value::Text("STARRED".into()),
        )]))
        .with_selector(draft_args(&[("id", Value::Text("mA".into()))]));
    use qfs_runtime::SharedApplier;
    driver_a.gmail_applier().apply_shared(&node).unwrap();

    assert_eq!(
        mock_a.recorded(),
        vec![RecordedCall::ModifyLabels {
            id: "mA".to_string(),
            add: vec!["STARRED".to_string()],
            remove: vec![],
        }]
    );
    assert!(
        mock_b.recorded().is_empty(),
        "account B's client was untouched"
    );
    let _ = driver_b; // bound to prove the two are distinct instances.
}

#[test]
fn describe_declares_the_child_address_per_node() {
    // 番地の鍵の宣言 (plan.md, settled 2026-07-18): the driver states the identity that
    // selects a child; a consumer never guesses.
    let (d, _) = driver_with_mock();
    // A label's rows are messages selected by `id` → `/mail/INBOX/@<id>`.
    assert_eq!(
        d.describe(&Path::new("/mail/INBOX")).unwrap().child_address,
        qfs_driver::ChildAddress::Key {
            columns: vec!["id".to_string()]
        }
    );
    // The root (and the label-management collection) lists labels: the label NAME is the
    // containment segment itself (`/mail/INBOX`), not an `@` selection.
    for root in ["/mail", "/mail/labels"] {
        assert_eq!(
            d.describe(&Path::new(root)).unwrap().child_address,
            qfs_driver::ChildAddress::EntryName {
                column: "name".to_string()
            }
        );
    }
    // A message (and its attachment/reply leaves) declares no child — relation segments
    // (`/@<id>/thread`) are a later phase, and "no child" is a valid, declared answer.
    assert_eq!(
        d.describe(&Path::new("/mail/INBOX/197abc"))
            .unwrap()
            .child_address,
        qfs_driver::ChildAddress::None
    );
}
