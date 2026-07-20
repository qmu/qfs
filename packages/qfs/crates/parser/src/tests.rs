//! Unit tests for the full blueprint §3 grammar (t04): one per pipe op, the governance
//! rejection cases, span-bearing errors, and the closed-core variant-count locks.

use super::*;
use crate::ast::*;

fn parse_ok(src: &str) -> Statement {
    parse_statement(src).unwrap_or_else(|e| panic!("expected `{src}` to parse, got {e}"))
}

fn parse_err(src: &str) -> ParseError {
    parse_statement(src).unwrap_err()
}

// ---- query pipeline + each pipe op ---------------------------------------

#[test]
fn from_only_query() {
    let stmt = parse_ok("/mail/inbox");
    let Statement::Query(p) = stmt else {
        panic!("expected Query")
    };
    let Source::Path(path) = p.source else {
        panic!("expected path source")
    };
    assert_eq!(path.segments.len(), 2);
    assert_eq!(path.segments[0].name, "mail");
    assert_eq!(path.segments[1].name, "inbox");
    assert!(p.ops.is_empty());
}

#[test]
fn multi_op_pipeline_covers_where_select_join_aggregate_order_limit() {
    let stmt = parse_ok(
        "/mail/inbox \
         |> WHERE subject LIKE 'invoice' AND size > 1000 \
         |> SELECT id, subject AS title \
         |> JOIN /contacts ON id == contact_id \
         |> AGGREGATE count(id) AS n \
         |> GROUP BY contact_id \
         |> ORDER BY n DESC \
         |> LIMIT 10",
    );
    let Statement::Query(p) = stmt else {
        panic!("expected Query")
    };
    assert_eq!(p.ops.len(), 7);
    assert!(matches!(p.ops[0], PipeOp::Where(_)));
    assert!(matches!(p.ops[1], PipeOp::Select(_)));
    assert!(matches!(p.ops[2], PipeOp::Join(_)));
    assert!(matches!(p.ops[3], PipeOp::Aggregate(_)));
    assert!(matches!(p.ops[4], PipeOp::GroupBy(_)));
    let PipeOp::OrderBy(ref keys) = p.ops[5] else {
        panic!("expected OrderBy")
    };
    assert!(keys[0].descending);
    assert_eq!(p.ops[6], PipeOp::Limit(10));
}

#[test]
fn extend_and_set_ops() {
    let stmt = parse_ok("/t |> EXTEND total = price |> SET flag = TRUE");
    let Statement::Query(p) = stmt else { panic!() };
    assert!(matches!(p.ops[0], PipeOp::Extend(_)));
    assert!(matches!(p.ops[1], PipeOp::Set(_)));
}

#[test]
fn distinct_op() {
    let stmt = parse_ok("/t |> DISTINCT");
    let Statement::Query(p) = stmt else { panic!() };
    assert_eq!(p.ops, vec![PipeOp::Distinct]);
}

#[test]
fn expand_op_struct_path() {
    let stmt = parse_ok("/mail/inbox |> EXPAND attachments");
    let Statement::Query(p) = stmt else { panic!() };
    assert_eq!(p.ops, vec![PipeOp::Expand(vec!["attachments".to_string()])]);
}

#[test]
fn union_except_intersect_ops() {
    for (src, want) in [
        ("/a |> UNION /b", "union"),
        ("/a |> EXCEPT /b", "except"),
        ("/a |> INTERSECT /b", "intersect"),
    ] {
        let stmt = parse_ok(src);
        let Statement::Query(p) = stmt else { panic!() };
        let got = match &p.ops[0] {
            PipeOp::Union(_) => "union",
            PipeOp::Except(_) => "except",
            PipeOp::Intersect(_) => "intersect",
            _ => "other",
        };
        assert_eq!(got, want, "for `{src}`");
    }
}

#[test]
fn as_op_names_relation() {
    let stmt = parse_ok("/t |> AS recent");
    let Statement::Query(p) = stmt else { panic!() };
    assert_eq!(p.ops, vec![PipeOp::As("recent".to_string())]);
}

// ---- codec registry seam --------------------------------------------------

#[test]
fn decode_and_encode_codecs() {
    let stmt = parse_ok("/fs/data.json |> DECODE json |> ENCODE yaml");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Decode(ref d) = p.ops[0] else {
        panic!("expected Decode")
    };
    assert_eq!(d.fmt, "json");
    let PipeOp::Encode(ref e) = p.ops[1] else {
        panic!("expected Encode")
    };
    assert_eq!(e.fmt, "yaml");
}

// ---- transform stage (blueprint §15, decision W) --------------------------

#[test]
fn transform_stage_parses_as_a_contextual_ident() {
    // `transform` is a CONTEXTUAL identifier (not a frozen keyword): it parses in pipe-stage
    // position, naming a declared definition, and composes with the keyword stages around it.
    let stmt = parse_ok("/mail/inbox |> transform triage |> ORDER BY priority");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Transform(ref t) = p.ops[0] else {
        panic!("expected Transform, got {:?}", p.ops[0])
    };
    assert_eq!(t.name, "triage");
    assert!(
        matches!(p.ops[1], PipeOp::OrderBy(_)),
        "a keyword stage still parses after transform"
    );
}

#[test]
fn transform_keyword_is_not_reserved() {
    // Because `transform` is a contextual ident, it remains usable as an ordinary identifier
    // elsewhere (here as a LET binding name) — proof it did not enter the frozen keyword set.
    let stmt = parse_ok(
        "LET transform = /sql/pg/rows |> WHERE active == true\n\
         transform |> SELECT id",
    );
    let Statement::Let { name, .. } = stmt else {
        panic!("expected LET, got {stmt:?}")
    };
    assert_eq!(name, "transform");
}

#[test]
fn transform_selector_is_a_bare_name_never_a_path() {
    // §5.5 lock — paths are data, names are definitions. A transform is referenced by its BARE
    // name; a `/transform/…` PATH in selector position is a category error and must NOT parse.
    // (qfs has no `FROM` keyword: a query's source is a bare path, so `/x` stands in for `FROM /x`.)
    let stmt = parse_ok("/x |> transform triage");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Transform(ref t) = p.ops[0] else {
        panic!("expected Transform, got {:?}", p.ops[0])
    };
    assert_eq!(t.name, "triage");
    // A path in selector position is rejected: `transform_op` reads a bare ident, so a `Token::Path`
    // there fails to parse. This LOCKS the category error out at the grammar.
    let _ = parse_err("/x |> transform /transform/triage");
}

// ---- `of <type>` use-site assertion (blueprint §5.6) -----------------------

#[test]
fn of_stage_parses_a_named_target() {
    // `of` is a CONTEXTUAL identifier (not a frozen keyword): it parses mid-pipe, naming a declared
    // type, and composes with the keyword stages around it. The name canonicalizes to `/type/<name>`.
    let stmt = parse_ok("/sql/shop/customers |> of customer |> SELECT id");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Of(ref o) = p.ops[0] else {
        panic!("expected Of, got {:?}", p.ops[0])
    };
    assert_eq!(o.target, OfTarget::Named("/type/customer".to_string()));
    assert!(
        matches!(p.ops[1], PipeOp::Select(_)),
        "a keyword stage still parses after of"
    );
}

#[test]
fn of_stage_parses_a_qualified_named_target() {
    // A qualified name (`chatwork/message`) canonicalizes under the `/type` mount, mirroring the
    // `create type`/`OF` name grammar.
    let stmt = parse_ok("/x |> of chatwork/message");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Of(ref o) = p.ops[0] else {
        panic!("expected Of, got {:?}", p.ops[0])
    };
    assert_eq!(
        o.target,
        OfTarget::Named("/type/chatwork/message".to_string())
    );
}

#[test]
fn of_stage_parses_an_inline_structural_literal() {
    // The §5.6 inline form: `of (priority text, reason text)` — an anonymous structural type literal
    // reusing the `CREATE TABLE` column-list production. This is the model-flavoured twin used as
    // `transform triage |> of (…)`.
    let stmt =
        parse_ok("/mail/inbox |> transform triage |> of (priority text, reason text NOT NULL)");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Of(ref o) = p.ops[1] else {
        panic!("expected Of, got {:?}", p.ops[1])
    };
    let OfTarget::Inline(cols) = &o.target else {
        panic!("expected inline target, got {:?}", o.target)
    };
    assert_eq!(cols.len(), 2);
    assert_eq!(cols[0].name, "priority");
    assert_eq!(cols[0].ty, "text");
    assert!(cols[0].nullable);
    assert_eq!(cols[1].name, "reason");
    assert!(!cols[1].nullable, "NOT NULL parsed onto the inline column");
}

#[test]
fn of_target_is_a_name_never_a_path() {
    // §5.5/§5.7 lock — paths are data, names are definitions. A `/type/…` PATH in `of` target
    // position is a category error: the named branch reads a `type_name` (begins with an `ident`,
    // so a `Token::Path` fails) and the inline branch requires `(`, so `of /type/x` parses as
    // neither — exactly like `transform /path`.
    let _ = parse_err("/x |> of /type/customer");
}

// ---- switch stage (blueprint §18) ------------------------------------------

#[test]
fn switch_stage_parses_the_blueprint_triage_example() {
    // The §18-B ruled example: three arms — a write arm with a leading op, a terminal-CALL arm
    // (whose `=>` named arg and parens must not confuse the arm boundary scan), and an `else`
    // write arm whose select carries a comma-separated projection list.
    let stmt = parse_ok(
        "/mail/inbox |> select id, subject, body \
           |> transform triage \
           |> switch route { \
                'urgent'  => select subject |> INSERT INTO /slack/ops-alerts, \
                'archive' => select id |> CALL mail.relabel(label => 'archived'), \
                else      => select id, subject |> INSERT INTO /mail/drafts \
              }",
    );
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Switch(ref s) = p.ops[2] else {
        panic!("expected Switch, got {:?}", p.ops[2])
    };
    assert_eq!(s.discriminant, "route");
    assert_eq!(s.arms.len(), 3);

    assert_eq!(s.arms[0].label.as_deref(), Some("urgent"));
    assert_eq!(s.arms[0].ops.len(), 1);
    assert!(matches!(s.arms[0].ops[0], PipeOp::Select(_)));
    let w0 = s.arms[0].write.as_ref().expect("write arm");
    assert_eq!(w0.verb, EffectVerb::Insert);
    assert_eq!(w0.target.segments[0].name, "slack");

    assert_eq!(s.arms[1].label.as_deref(), Some("archive"));
    assert_eq!(s.arms[1].ops.len(), 2, "select + terminal CALL");
    let PipeOp::Call(ref c) = s.arms[1].ops[1] else {
        panic!("expected terminal CALL arm, got {:?}", s.arms[1].ops)
    };
    assert_eq!(c.driver, "mail");
    assert_eq!(c.action, "relabel");
    assert!(s.arms[1].write.is_none());

    assert_eq!(s.arms[2].label, None, "else arm");
    let PipeOp::Select(ref projs) = s.arms[2].ops[0] else {
        panic!("expected select in else arm")
    };
    assert_eq!(
        projs.len(),
        2,
        "the projection-list comma stays inside the arm; the arm boundary scan splits only at \
         a `'label' =>` / `else =>` comma"
    );
    assert!(s.arms[2].write.is_some());
}

#[test]
fn switch_arm_can_be_a_bare_write() {
    // The §18-B compact form: an arm with no leading ops is just the write.
    let stmt = parse_ok(
        "/drive/Inbox/contracts |> transform classify \
           |> switch kind { 'invoice' => INSERT INTO /sql/books/invoices, \
                            else      => UPSERT INTO /drive/Inbox/unsorted }",
    );
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Switch(ref s) = p.ops[1] else {
        panic!("expected Switch")
    };
    assert!(s.arms[0].ops.is_empty());
    assert_eq!(
        s.arms[0].write.as_ref().map(|w| w.verb),
        Some(EffectVerb::Insert)
    );
    assert!(s.arms[1].ops.is_empty());
    assert_eq!(
        s.arms[1].write.as_ref().map(|w| w.verb),
        Some(EffectVerb::Upsert)
    );
}

#[test]
fn switch_arm_write_carries_returning() {
    let stmt = parse_ok("/t |> switch c { else => INSERT INTO /x RETURNING id, name }");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Switch(ref s) = p.ops[0] else {
        panic!("expected Switch")
    };
    let w = s.arms[0].write.as_ref().expect("write arm");
    assert_eq!(w.returning.as_ref().map(Vec::len), Some(2));
}

#[test]
fn switch_pure_arm_projection_comma_does_not_swallow_the_next_arm() {
    // The grammar hazard the bounded arm scan exists for: a (shape-wise) pure arm ending in a
    // multi-projection select, directly followed by another arm. Without the bound, the greedy
    // projection list would consume `, 'b'` as a third projection and the parse would break.
    let stmt = parse_ok("/t |> switch c { 'a' => select id, name, else => INSERT INTO /x }");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Switch(ref s) = p.ops[0] else {
        panic!("expected Switch")
    };
    assert_eq!(s.arms.len(), 2);
    let PipeOp::Select(ref projs) = s.arms[0].ops[0] else {
        panic!("expected select arm")
    };
    assert_eq!(projs.len(), 2, "`id, name` stays in the first arm");
    assert_eq!(s.arms[1].label, None);
}

#[test]
fn switch_parses_mid_pipe_and_in_effect_body_shape_only() {
    // The parser validates SHAPE only — terminal-position enforcement is a structured eval
    // error (`switch_not_terminal`), so these parse and fail later with a diagnostic that can
    // name the context instead of a token position.
    let stmt = parse_ok("/t |> switch c { else => INSERT INTO /x } |> LIMIT 1");
    let Statement::Query(p) = stmt else { panic!() };
    assert!(matches!(p.ops[0], PipeOp::Switch(_)));
    assert_eq!(p.ops[1], PipeOp::Limit(1));
}

#[test]
fn switch_and_else_are_not_reserved_keywords() {
    // Contextual idents (the `transform` lesson): both remain usable as ordinary identifiers,
    // proof the frozen keyword set did not move (39 stays 39).
    let stmt = parse_ok(
        "LET switch = /sql/pg/rows |> WHERE active == true\n\
         switch |> SELECT id",
    );
    let Statement::Let { name, .. } = stmt else {
        panic!("expected LET, got {stmt:?}")
    };
    assert_eq!(name, "switch");
    let stmt = parse_ok("/t |> SELECT else");
    let Statement::Query(p) = stmt else { panic!() };
    assert!(matches!(p.ops[0], PipeOp::Select(_)));
}

#[test]
fn switch_malformed_arms_are_parse_errors() {
    // A missing `=>`, an empty body, a write followed by more ops (a write is terminal), and a
    // missing brace all fail at parse with a cut (committed) error.
    let _ = parse_err("/t |> switch c { 'a' INSERT INTO /x }");
    let _ = parse_err("/t |> switch c { 'a' => }");
    let _ = parse_err("/t |> switch c { 'a' => INSERT INTO /x |> select id }");
    let _ = parse_err("/t |> switch c { 'a' => INSERT INTO /x");
}

// ---- procedure registry seam ---------------------------------------------

#[test]
fn call_proc_with_named_and_positional_args() {
    let stmt =
        parse_ok("/mail/drafts |> CALL mail.send |> CALL github.merge(method => 'squash', 42)");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Call(ref c0) = p.ops[0] else {
        panic!("expected Call")
    };
    assert_eq!(c0.driver, "mail");
    assert_eq!(c0.action, "send");
    assert!(c0.args.is_empty());
    let PipeOp::Call(ref c1) = p.ops[1] else {
        panic!("expected Call")
    };
    assert_eq!(c1.driver, "github");
    assert_eq!(c1.action, "merge");
    assert_eq!(c1.args.len(), 2);
    assert_eq!(c1.args[0].name.as_deref(), Some("method"));
    assert_eq!(c1.args[1].name, None);
}

// ---- function registry seam ----------------------------------------------

#[test]
fn registry_fn_in_expression() {
    let stmt = parse_ok("/t |> SELECT upper(name)");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Select(ref projs) = p.ops[0] else {
        panic!()
    };
    let Projection::Expr { expr, .. } = &projs[0] else {
        panic!()
    };
    let Expr::Fn(f) = expr else {
        panic!("expected Fn, got {expr:?}")
    };
    assert_eq!(f.name, "upper");
    assert_eq!(f.args.len(), 1);
}

// ---- path registry seam: @version, AS OF, struct access -------------------

#[test]
fn path_at_version_is_preserved() {
    let stmt = parse_ok("/git/repo@main/src");
    let Statement::Query(p) = stmt else { panic!() };
    let Source::Path(path) = p.source else {
        panic!()
    };
    let seg = path.segments.iter().find(|s| s.name == "repo").unwrap();
    assert_eq!(seg.version.as_deref(), Some("main"));
}

#[test]
fn quoted_path_segment_reaches_the_ast_as_a_raw_name() {
    // Ticket 20260717120200. Quoting is purely LEXICAL: the AST (and therefore the `/seg/seg`
    // string every driver re-splits) carries the raw name, with no quotes and no glob flag. That
    // is what lets drivers keep splitting on `/` unchanged.
    let stmt = parse_ok("/drive/my/Reports/'Q3 budget (final)?.xlsx'");
    let Statement::Query(p) = stmt else { panic!() };
    let Source::Path(path) = p.source else {
        panic!()
    };
    let names: Vec<&str> = path.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["drive", "my", "Reports", "Q3 budget (final)?.xlsx"]
    );
    assert!(
        path.segments.iter().all(|s| !s.glob),
        "a quoted `?` is a literal character, so no segment globs"
    );
}

#[test]
fn quoted_path_segment_addresses_a_single_file_remove() {
    // The spelling the incident could not write, which forced the `where name == '…'` detour
    // onto the over-delete bug (ticket 20260717102000).
    let stmt = parse_ok("remove /drive/my/'Q3 budget?.xlsx'");
    let Statement::Effect(e) = stmt else { panic!() };
    assert_eq!(target_path(&e), "/drive/my/Q3 budget?.xlsx");
}

#[test]
fn selection_segment_reaches_the_ast_flagged() {
    // 番地の`@選択` (plan.md, settled 2026-07-18): `/mail/INBOX/@<id>` parses with the final
    // segment FLAGGED as a selection carrying the raw key text — never a containment name.
    // The parser carries shape only; the single lowering site turns it into the where step.
    let stmt = parse_ok("/mail/INBOX/@197a2b3c |> SELECT subject");
    let Statement::Query(p) = stmt else { panic!() };
    let Source::Path(path) = p.source else {
        panic!()
    };
    assert_eq!(path.segments.len(), 3);
    let sel = &path.segments[2];
    assert!(sel.selection, "the `@` segment is a selection");
    assert_eq!(sel.name, "197a2b3c", "raw key text, `@` stripped");
    assert!(sel.version.is_none() && !sel.glob);
    assert!(
        path.segments[..2].iter().all(|s| !s.selection),
        "containment segments stay unflagged"
    );
    // Composite spelling: the raw comma-joined values ride one segment.
    let stmt = parse_ok("/sql/crm/invoices/@2024,INV-003");
    let Statement::Query(p) = stmt else { panic!() };
    let Source::Path(path) = p.source else {
        panic!()
    };
    assert_eq!(path.segments[3].name, "2024,INV-003");
    assert!(path.segments[3].selection);
}

#[test]
fn path_as_of_temporal() {
    let stmt = parse_ok("/sql/pg/orders AS OF '2026-01-01'");
    let Statement::Query(p) = stmt else { panic!() };
    let Source::Path(path) = p.source else {
        panic!()
    };
    assert_eq!(path.as_of.as_deref(), Some("2026-01-01"));
}

#[test]
fn struct_path_access_a_b_c() {
    let stmt = parse_ok("/t |> WHERE a.b.c == 1");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Where(Expr::Binary { lhs, .. }) = &p.ops[0] else {
        panic!()
    };
    assert_eq!(
        **lhs,
        Expr::Path(vec!["a".to_string(), "b".to_string(), "c".to_string()])
    );
}

// ---- expression operators -------------------------------------------------

#[test]
fn in_between_anyop_predicates() {
    let stmt =
        parse_ok("/t |> WHERE id IN (1, 2, 3) AND price BETWEEN 10 AND 20 AND x == ANY (4, 5)");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Where(e) = &p.ops[0] else {
        panic!()
    };
    // Top of the AND chain holds the AnyOp on its rhs.
    let Expr::Binary {
        op: Op::And, rhs, ..
    } = e
    else {
        panic!("expected AND at top, got {e:?}")
    };
    assert!(matches!(**rhs, Expr::AnyOp { .. }), "rhs should be AnyOp");
}

#[test]
fn not_and_or_precedence() {
    let stmt = parse_ok("/t |> WHERE NOT a == 1 OR b == 2");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Where(e) = &p.ops[0] else {
        panic!()
    };
    // OR binds loosest: top node is OR.
    assert!(matches!(e, Expr::Binary { op: Op::Or, .. }));
}

// ---- effect statements ----------------------------------------------------

#[test]
fn insert_values_returning() {
    let stmt =
        parse_ok("INSERT INTO /mail/drafts VALUES (to, subject) ('a@b.c', 'hi') RETURNING id");
    let Statement::Effect(e) = stmt else {
        panic!("expected Effect")
    };
    assert_eq!(e.verb, EffectVerb::Insert);
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values body")
    };
    assert_eq!(
        v.columns.as_ref().unwrap(),
        &vec!["to".to_string(), "subject".to_string()]
    );
    assert_eq!(v.rows.len(), 1);
    assert!(e.returning.is_some());
}

#[test]
fn insert_draft_with_array_struct_bytes_attachment_literal() {
    // t92: an `[ { filename, mime, bytes: X'…' } ]` attachments column parses end-to-end (the
    // foundation for a Gmail draft attachment). `X'68656c6c6f'` decodes to "hello".
    let stmt = parse_ok(
        "INSERT INTO /mail/drafts VALUES ('to@x.y', 'Subject', 'Body', \
         [ { filename: 'note.txt', mime: 'text/plain', bytes: X'68656c6c6f' } ])",
    );
    let Statement::Effect(e) = stmt else {
        panic!("expected Effect")
    };
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values body")
    };
    let row = &v.rows[0];
    assert_eq!(row.len(), 4);
    let Expr::Array(items) = &row[3] else {
        panic!("expected an array constructor, got {:?}", row[3])
    };
    assert_eq!(items.len(), 1);
    let Expr::Struct(fields) = &items[0] else {
        panic!("expected a struct constructor element")
    };
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].0, "filename");
    assert!(matches!(&fields[0].1, Expr::Lit(Literal::Str(s)) if s == "note.txt"));
    assert_eq!(fields[2].0, "bytes");
    assert!(matches!(&fields[2].1, Expr::Lit(Literal::Bytes(b)) if b == b"hello"));
}

#[test]
fn empty_array_and_struct_literals_parse() {
    let Statement::Effect(e) = parse_ok("INSERT INTO /t VALUES ([], {})") else {
        panic!("expected Effect")
    };
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values body")
    };
    assert!(matches!(&v.rows[0][0], Expr::Array(a) if a.is_empty()));
    assert!(matches!(&v.rows[0][1], Expr::Struct(s) if s.is_empty()));
}

#[test]
fn bad_hex_bytes_literal_is_rejected() {
    // A non-hex digit or an odd number of digits is a lex error surfaced as a parse failure.
    let _ = parse_err("INSERT INTO /t VALUES (X'zz')");
    let _ = parse_err("INSERT INTO /t VALUES (X'abc')");
}

#[test]
fn upsert_distinct_from_insert() {
    let stmt = parse_ok("UPSERT INTO /s3/bucket/key VALUES ('blob')");
    let Statement::Effect(e) = stmt else { panic!() };
    assert_eq!(e.verb, EffectVerb::Upsert);
}

#[test]
fn insert_from_subpipeline() {
    let stmt = parse_ok("INSERT INTO /archive /mail/inbox |> WHERE flag == TRUE");
    let Statement::Effect(e) = stmt else { panic!() };
    assert!(matches!(e.body, EffectBody::Pipeline(_)));
}

#[test]
fn update_set_where() {
    let stmt = parse_ok("UPDATE /sql/pg/orders SET status = 'done' WHERE id == 7");
    let Statement::Effect(e) = stmt else { panic!() };
    assert_eq!(e.verb, EffectVerb::Update);
    let EffectBody::SetWhere { set, filter } = &e.body else {
        panic!()
    };
    assert_eq!(set.len(), 1);
    assert!(filter.is_some());
}

#[test]
fn remove_where() {
    let stmt = parse_ok("REMOVE /mail/spam WHERE age > 30");
    let Statement::Effect(e) = stmt else { panic!() };
    assert_eq!(e.verb, EffectVerb::Remove);
}

// ---- writes as pipeline stages (decision Q, t72) --------------------------

/// Render an AST to canonical JSON with every byte-`span` field stripped, so two spellings of
/// the SAME write — which sit at different byte offsets — compare on *structure* alone. The
/// whole point of t72 is that the pipe-stage and verb-leading forms lower to one `EffectStmt`;
/// spans are the only thing that legitimately differs between two source strings.
fn ast_without_spans(stmt: &Statement) -> serde_json::Value {
    let mut v = serde_json::to_value(stmt).expect("AST serializes");
    strip_spans(&mut v);
    v
}

fn strip_spans(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            map.remove("span");
            for child in map.values_mut() {
                strip_spans(child);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                strip_spans(child);
            }
        }
        _ => {}
    }
}

/// Decision Q, the load-bearing invariant: a pipe-stage write parses to the SAME effect AST the
/// verb-leading spelling produces — one plan, two renderings — for every write verb.
#[test]
fn pipe_stage_writes_match_their_verb_leading_form() {
    // INSERT/UPSERT: the upstream pipeline IS the inserted relation (the verb-leading form's
    // sub-pipeline body); target stays explicit in the stage.
    let pairs = [
        (
            "/mail/inbox |> WHERE flag == TRUE |> INSERT INTO /archive",
            "INSERT INTO /archive /mail/inbox |> WHERE flag == TRUE",
        ),
        (
            "/sql/pg/src |> SELECT id, body |> UPSERT INTO /drive/out.csv",
            "UPSERT INTO /drive/out.csv /sql/pg/src |> SELECT id, body",
        ),
        // UPDATE/REMOVE: target + filter are lifted from the upstream `path |> WHERE …` so the
        // pipe-stage form reproduces the verb-leading `UPDATE <path> SET … WHERE …` exactly.
        (
            "/sql/pg/orders |> WHERE id == 7 |> UPDATE SET status = 'done'",
            "UPDATE /sql/pg/orders SET status = 'done' WHERE id == 7",
        ),
        (
            "/mail/spam |> WHERE age > 30 |> REMOVE",
            "REMOVE /mail/spam WHERE age > 30",
        ),
        // RETURNING rides as a trailing `|>` stage in the pipe-stage form, inline in the other.
        (
            "/sql/pg/orders |> WHERE id == 7 |> UPDATE SET status = 'done' |> RETURNING id",
            "UPDATE /sql/pg/orders SET status = 'done' WHERE id == 7 RETURNING id",
        ),
    ];
    for (pipe_form, verb_form) in pairs {
        let lhs = ast_without_spans(&parse_ok(pipe_form));
        let rhs = ast_without_spans(&parse_ok(verb_form));
        assert_eq!(
            lhs, rhs,
            "pipe-stage `{pipe_form}` must lower to the same effect AST as `{verb_form}`"
        );
    }
}

/// Both forms are legal and produce an `Effect`; the source-less `VALUES` literal is unchanged.
#[test]
fn both_write_forms_are_legal() {
    // Pipe-stage write.
    let Statement::Effect(e) = parse_ok("/mail/inbox |> WHERE flag == TRUE |> REMOVE") else {
        panic!("pipe-stage REMOVE must be an Effect")
    };
    assert_eq!(e.verb, EffectVerb::Remove);
    // Source-less verb-leading literal still parses (the only INSERT form with no inflow).
    let Statement::Effect(lit) = parse_ok("INSERT INTO /mail/drafts VALUES ('hi')") else {
        panic!("source-less INSERT … VALUES must stay legal")
    };
    assert!(matches!(lit.body, EffectBody::Values(_)));
}

/// A pipe-stage INSERT/UPSERT keeps the upstream as the relational body (not a `SET`/`VALUES`).
#[test]
fn pipe_stage_insert_body_is_the_upstream_pipeline() {
    let Statement::Effect(e) = parse_ok("/sql/pg/src |> ENCODE csv |> UPSERT INTO /drive/o.csv")
    else {
        panic!()
    };
    assert_eq!(e.verb, EffectVerb::Upsert);
    let EffectBody::Pipeline(p) = &e.body else {
        panic!("expected the upstream pipeline as the body")
    };
    assert!(matches!(p.source, Source::Path(_)));
    assert_eq!(p.ops.len(), 1, "the ENCODE stage is part of the body");
}

/// A write stage with NO upstream source (a bare `|> INSERT INTO …`) is a clear error, not a
/// silent acceptance — decision Q's "anything that is neither legal form is an error".
#[test]
fn bare_pipe_write_with_no_source_is_an_error() {
    let _ = parse_err("|> INSERT INTO /mail/drafts");
    let _ = parse_err("|> REMOVE");
}

/// A pipe-stage UPDATE/REMOVE whose upstream is not a plain `path |> WHERE …` (here a `SELECT`
/// transform precedes it) cannot lower to the verb-leading in-place form, so it is rejected.
#[test]
fn pipe_stage_update_rejects_non_liftable_upstream() {
    let _ = parse_err("/sql/pg/orders |> SELECT id |> UPDATE SET status = 'done'");
}

/// A pipe-stage write is still an explicit effect under `PREVIEW`/`COMMIT` (the safety floor is
/// unchanged — only the surface syntax moved).
#[test]
fn pipe_stage_write_wraps_in_a_plan() {
    let Statement::Plan(p) = parse_ok("COMMIT /mail/spam |> WHERE age > 30 |> REMOVE") else {
        panic!("expected a plan wrapper")
    };
    assert!(p.commit);
    assert!(matches!(p.inner.as_ref(), Statement::Effect(_)));
}

// ---- server DDL -----------------------------------------------------------

#[test]
fn ddl_endpoint_as_query() {
    let stmt = parse_ok("CREATE ENDPOINT recent ON 'GET /recent' AS /mail/inbox |> LIMIT 5");
    let Statement::Ddl(d) = stmt else {
        panic!("expected Ddl")
    };
    assert_eq!(d.kind, DdlKind::Endpoint);
    assert_eq!(d.name, "recent");
    assert_eq!(d.target, vec!["server", "endpoints", "recent"]);
    assert!(d.as_query.is_some());
    assert_eq!(d.on.as_deref(), Some("GET /recent"));
}

#[test]
fn create_connection_is_retired_and_points_at_connect() {
    // `CREATE CONNECTION` is retired (the "declared drivers are the normal way" mission): the sole
    // declaration statement is `CONNECT /<family>/<name> TO <driver> …`, persisted in the
    // `path_binding` registry. Every old form now parse-errors, and the message steers the author to
    // `CONNECT` so a stale `connections.qfs` line fails loudly with a migration hint.
    for src in [
        "CREATE CONNECTION analytics DRIVER postgres AT 'postgres://db/analytics' SECRET 'env:PG_PASSWORD'",
        "CREATE CONNECTION orders DRIVER sqlite AT '/data/orders.db'",
        "CREATE CONNECTION work DRIVER gmail SECRET 'vault:gmail/work'",
    ] {
        let err = parse_err(src);
        assert!(
            err.message.contains("CREATE CONNECTION is retired") && err.message.contains("CONNECT"),
            "expected a retired-CONNECTION pointer for `{src}`, got: {err}"
        );
    }
}

// ---- CONNECT / DISCONNECT — defined-path bindings (EPIC 20260701100000) -----

/// Pull the desugared `EffectStmt` out of a CONNECT/DISCONNECT parse.
fn effect_of(stmt: Statement) -> EffectStmt {
    match stmt {
        Statement::Effect(e) => e,
        other => panic!("expected an Effect (the CONNECT desugar), got {other:?}"),
    }
}

/// The canonical `/a/b/c` path of an effect target.
fn target_path(e: &EffectStmt) -> String {
    let mut s = String::new();
    for seg in &e.target.segments {
        s.push('/');
        s.push_str(&seg.name);
    }
    s
}

/// Read a named column's string value from a single-row `VALUES` body (`None` for a NULL cell).
fn values_cell(e: &EffectStmt, col: &str) -> Option<String> {
    let EffectBody::Values(v) = &e.body else {
        panic!("expected a VALUES body")
    };
    let cols = v.columns.as_ref().expect("explicit columns");
    let idx = cols.iter().position(|c| c == col).expect("column present");
    match &v.rows[0][idx] {
        Expr::Lit(Literal::Str(s)) => Some(s.clone()),
        Expr::Lit(Literal::Null) => None,
        other => panic!("unexpected cell {other:?}"),
    }
}

#[test]
fn connect_full_desugars_to_a_sys_paths_upsert() {
    // The headline form: bind `/work/orders` to a postgres connection with an AT locator + a SECRET
    // reference. CONNECT is SUGAR over `UPSERT INTO /sys/paths` — no new Statement variant.
    let e = effect_of(parse_ok(
        "CONNECT /work/orders TO postgres AT 'postgres://db/orders' SECRET 'env:PG_PASS'",
    ));
    assert_eq!(e.verb, EffectVerb::Insert);
    assert_eq!(target_path(&e), "/sys/paths");
    assert_eq!(values_cell(&e, "path").as_deref(), Some("/work/orders"));
    assert_eq!(values_cell(&e, "driver").as_deref(), Some("postgres"));
    assert_eq!(
        values_cell(&e, "at").as_deref(),
        Some("postgres://db/orders")
    );
    assert_eq!(
        values_cell(&e, "secret_ref").as_deref(),
        Some("env:PG_PASS")
    );
    assert_eq!(values_cell(&e, "alias_of"), None);
}

#[test]
fn connect_multi_segment_path_binds() {
    // A recursive/nested defined path (`/team/finance/ledger`) is just a longer `path` value.
    let e = effect_of(parse_ok(
        "CONNECT /team/finance/ledger TO sqlite AT '/data/l.db'",
    ));
    assert_eq!(
        values_cell(&e, "path").as_deref(),
        Some("/team/finance/ledger")
    );
    assert_eq!(values_cell(&e, "driver").as_deref(), Some("sqlite"));
    assert_eq!(values_cell(&e, "secret_ref"), None);
}

#[test]
fn connect_secret_before_at_is_order_independent() {
    // AT/SECRET are collected in any order (the sugar-shape clause loop).
    let e = effect_of(parse_ok(
        "CONNECT /m TO gmail SECRET 'vault:gmail/work' ACCOUNT 'a@qmu.jp' APP 'client'",
    ));
    assert_eq!(values_cell(&e, "driver").as_deref(), Some("gmail"));
    assert_eq!(values_cell(&e, "at"), None);
    assert_eq!(
        values_cell(&e, "secret_ref").as_deref(),
        Some("vault:gmail/work")
    );
    assert_eq!(values_cell(&e, "account").as_deref(), Some("a@qmu.jp"));
    assert_eq!(values_cell(&e, "app").as_deref(), Some("client"));
}

#[test]
fn connect_alias_reuses_a_connection() {
    // `TO /existing-path` (a leading-slash target) is the ALIAS arm: no driver/secret, an alias_of.
    let e = effect_of(parse_ok("CONNECT /db TO /work/orders"));
    assert_eq!(e.verb, EffectVerb::Insert);
    assert_eq!(target_path(&e), "/sys/paths");
    assert_eq!(values_cell(&e, "path").as_deref(), Some("/db"));
    assert_eq!(values_cell(&e, "alias_of").as_deref(), Some("/work/orders"));
    assert_eq!(values_cell(&e, "driver"), None);
}

#[test]
fn disconnect_desugars_to_a_sys_paths_remove() {
    // `DISCONNECT /<path>` → `REMOVE /sys/paths/<path>` (the user path rides as trailing segments).
    let e = effect_of(parse_ok("DISCONNECT /work/orders"));
    assert_eq!(e.verb, EffectVerb::Remove);
    assert_eq!(target_path(&e), "/sys/paths/work/orders");
    assert!(matches!(
        e.body,
        EffectBody::SetWhere { ref set, filter: None } if set.is_empty()
    ));
}

#[test]
fn connect_without_a_target_is_a_crisp_error() {
    // Committed after the verb: `CONNECT /x` with no `TO …` is a hard parse error, not a silent
    // fallthrough to a pipeline named `connect`.
    assert!(parse_statement("CONNECT /x").is_err());
    assert!(parse_statement("CONNECT").is_err());
    assert!(parse_statement("DISCONNECT").is_err());
}

#[test]
fn connect_and_disconnect_add_no_frozen_keyword() {
    // The additive-by-contextual-ident contract (the t31 AT lesson): CONNECT/DISCONNECT/TO are NOT
    // in the frozen keyword set — they are `word(...)` idents, exactly like CONNECTION/SECRET/AT.
    // ADR 0008 adds HOST/ACCOUNT/APP under the same contract.
    for w in ["connect", "disconnect", "to", "host", "account", "app"] {
        assert!(
            !KEYWORDS.contains(&w),
            "`{w}` must NOT be a frozen keyword (it is a contextual ident)"
        );
    }
}

// ---- CREATE ACCOUNT — in-language account declaration (20260703040000) -------

#[test]
fn create_account_desugars_to_a_sys_accounts_insert() {
    // `CREATE ACCOUNT <provider> '<label>'` is SUGAR over `INSERT INTO /sys/accounts` — no new
    // Statement variant (the `/sys/paths` precedent). Selectors only: provider + account, never a
    // token.
    let e = effect_of(parse_ok("CREATE ACCOUNT google 'a@qmu.jp' APP 'client'"));
    assert_eq!(e.verb, EffectVerb::Insert);
    assert_eq!(target_path(&e), "/sys/accounts");
    assert_eq!(values_cell(&e, "provider").as_deref(), Some("google"));
    assert_eq!(values_cell(&e, "account").as_deref(), Some("a@qmu.jp"));
    assert_eq!(values_cell(&e, "app").as_deref(), Some("client"));

    // A cloud provider + label round-trips the same way.
    let e = effect_of(parse_ok("CREATE ACCOUNT github 'work'"));
    assert_eq!(values_cell(&e, "provider").as_deref(), Some("github"));
    assert_eq!(values_cell(&e, "account").as_deref(), Some("work"));
    assert_eq!(values_cell(&e, "app"), None);
}

#[test]
fn create_account_needs_provider_and_label() {
    // Committed after the `ACCOUNT` noun: a missing label is a crisp error, not a fallthrough.
    assert!(parse_statement("CREATE ACCOUNT google").is_err());
    // `account` is NOT a frozen keyword — it is a contextual ident (asserted above), so the verb is
    // additive: `CREATE ACCOUNT …` parses without reserving a new keyword.
    assert!(!KEYWORDS.contains(&"account"));
}

#[test]
fn create_account_carries_a_secret_reference() {
    // 20260718203325: an optional `SECRET '<ref>'` rides as the `secret_ref` selector column of the
    // desugared `/sys/accounts` row — a reference resolved at USE, never an inline token.
    let e = effect_of(parse_ok("CREATE ACCOUNT cf 'mycf' SECRET 'env:CF_TOKEN'"));
    assert_eq!(target_path(&e), "/sys/accounts");
    assert_eq!(values_cell(&e, "provider").as_deref(), Some("cf"));
    assert_eq!(values_cell(&e, "account").as_deref(), Some("mycf"));
    assert_eq!(
        values_cell(&e, "secret_ref").as_deref(),
        Some("env:CF_TOKEN")
    );

    // APP and SECRET are order-independent, mirroring CONNECT's clause loop.
    let e = effect_of(parse_ok(
        "CREATE ACCOUNT cf 'mycf' SECRET 'vault:cf/mycf' APP 'client'",
    ));
    assert_eq!(
        values_cell(&e, "secret_ref").as_deref(),
        Some("vault:cf/mycf")
    );
    assert_eq!(values_cell(&e, "app").as_deref(), Some("client"));

    // Absent the clause, `secret_ref` is NULL (the token stays sealed out-of-band).
    let e = effect_of(parse_ok("CREATE ACCOUNT github 'work'"));
    assert_eq!(values_cell(&e, "secret_ref"), None);
}

#[test]
fn create_account_rejects_an_inline_secret() {
    // References only (`env:`/`vault:`): an inline non-reference secret is a PARSE error, never
    // material sealed into the statement text.
    assert!(parse_statement("CREATE ACCOUNT cf 'mycf' SECRET 'hunter2'").is_err());
}

// ---- §13 declared drivers — CREATE DRIVER / TYPE / declared VIEW / MAP -------
// Each desugars to `INSERT INTO /sys/drivers` (the `/sys/paths` precedent); no new Statement
// variant. Scripts are credential-free: no AUTH form can carry a token value.

/// Read a boolean cell from a single-row `VALUES` body (the `irreversible` column).
fn values_bool(e: &EffectStmt, col: &str) -> bool {
    let EffectBody::Values(v) = &e.body else {
        panic!("expected a VALUES body")
    };
    let cols = v.columns.as_ref().expect("explicit columns");
    let idx = cols.iter().position(|c| c == col).expect("column present");
    match &v.rows[0][idx] {
        Expr::Lit(Literal::Bool(b)) => *b,
        other => panic!("expected a bool cell, got {other:?}"),
    }
}

#[test]
fn create_driver_desugars_to_a_sys_drivers_insert() {
    let e = effect_of(parse_ok(
        "CREATE DRIVER chatwork AT 'https://api.chatwork.com/v2' AUTH HEADER 'x-chatworktoken'",
    ));
    assert_eq!(e.verb, EffectVerb::Insert);
    assert_eq!(target_path(&e), "/sys/drivers");
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("driver"));
    assert_eq!(values_cell(&e, "name").as_deref(), Some("chatwork"));
    assert_eq!(
        values_cell(&e, "base_url").as_deref(),
        Some("https://api.chatwork.com/v2")
    );
    // The auth descriptor mirrors the shipped `AuthStrategy::Header { name }` — the header NAME
    // only, never a token.
    let auth = values_cell(&e, "auth").expect("auth present");
    assert!(
        auth.contains("\"header\"") && auth.contains("x-chatworktoken"),
        "{auth}"
    );
    assert_eq!(values_cell(&e, "pagination"), None);
}

#[test]
fn driver_auth_bearer_carries_no_token_value() {
    // The credential-free invariant: BEARER takes NO argument, so an inline token is a parse error
    // (the trailing string cannot attach anywhere) — a script is structurally unable to hold a secret.
    assert!(parse_statement("CREATE DRIVER x AT 'https://api.x.io' AUTH BEARER").is_ok());
    assert!(
        parse_statement("CREATE DRIVER x AT 'https://api.x.io' AUTH BEARER 'sk-secret-token'")
            .is_err(),
        "AUTH BEARER must reject an inline token value"
    );
    // HEADER's string is the header NAME, not a value — and there is no token-value clause at all.
    let e = effect_of(parse_ok(
        "CREATE DRIVER y AT 'https://api.y.io' AUTH BEARER",
    ));
    assert!(values_cell(&e, "auth").unwrap().contains("bearer"));
}

#[test]
fn create_driver_none_auth_and_oauth2_and_pagination() {
    // AUTH NONE defaults through when omitted, and is spellable explicitly.
    let none = effect_of(parse_ok("CREATE DRIVER pub AT 'https://api.pub.io'"));
    assert!(values_cell(&none, "auth").unwrap().contains("none"));

    // OAUTH2 declares endpoints + scopes (no secret), and PAGINATE CURSOR the follow descriptor.
    let e = effect_of(parse_ok(
        "CREATE DRIVER svc AT 'https://api.svc.io' \
         AUTH OAUTH2 (authorize 'https://svc.io/auth' token 'https://svc.io/token' scopes 'read write') \
         PAGINATE CURSOR (next 'next_cursor' param 'cursor' MAX 50)",
    ));
    let auth = values_cell(&e, "auth").unwrap();
    assert!(
        auth.contains("oauth2") && auth.contains("svc.io/token") && auth.contains("read write")
    );
    let page = values_cell(&e, "pagination").unwrap();
    assert!(page.contains("cursor") && page.contains("next_cursor") && page.contains("50"));

    // PAGINATE LINK is the RFC 5988 archetype.
    let link = effect_of(parse_ok(
        "CREATE DRIVER l AT 'https://api.l.io' PAGINATE LINK MAX 20",
    ));
    let lp = values_cell(&link, "pagination").unwrap();
    assert!(lp.contains("link") && lp.contains("20"));
}

#[test]
fn create_driver_auth_account_desugars_to_a_provider_reference() {
    // `AUTH ACCOUNT '<provider>'` desugars to a credential-free descriptor naming ONLY the provider
    // — the account-referenced auth that lets an OAuth-style service live in the declared model. No
    // token/secret appears in the row (it stays in the account/vault layer).
    let e = effect_of(parse_ok(
        "CREATE DRIVER ghdecl AT 'https://api.github.com' AUTH ACCOUNT 'github'",
    ));
    let auth = values_cell(&e, "auth").unwrap();
    assert!(
        auth.contains("account") && auth.contains("github"),
        "AUTH ACCOUNT descriptor names the provider: {auth}"
    );
    // Never a token/secret value in the descriptor.
    assert!(!auth.to_lowercase().contains("token") && !auth.contains("secret"));
}

#[test]
fn create_type_desugars_with_columns_as_json() {
    let e = effect_of(parse_ok(
        "CREATE TYPE chatwork/message (message_id text PRIMARY KEY, body text NOT NULL, send_time timestamp)",
    ));
    assert_eq!(target_path(&e), "/sys/drivers");
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("type"));
    assert_eq!(
        values_cell(&e, "name").as_deref(),
        Some("/type/chatwork/message")
    );
    let body = values_cell(&e, "body").expect("columns json");
    assert!(body.contains("message_id") && body.contains("primary_key"));
    assert!(body.contains("send_time") && body.contains("\"nullable\""));
    // §5.4: the body is now a JSON OBJECT with a `columns` array and a `where` predicate slot; a
    // type with no `WHERE` stores `"where": null`.
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("object body");
    assert!(parsed.get("columns").and_then(|c| c.as_array()).is_some());
    assert!(parsed.get("where").expect("where key").is_null());
}

#[test]
fn create_type_with_refinement_stores_the_predicate_expr() {
    let e = effect_of(parse_ok(
        "CREATE TYPE email (value text) WHERE value LIKE '%@%'",
    ));
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("type"));
    let body = values_cell(&e, "body").expect("object body");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("object body");
    // The `where` slot holds a non-null serialized `Expr` (a `Like` node over `value`).
    let refinement = parsed.get("where").expect("where key");
    assert!(!refinement.is_null(), "refinement is stored, not null");
    // The stored `Expr` rehydrates back to a `Like` predicate over the `value` column.
    let expr: crate::ast::Expr =
        serde_json::from_value(refinement.clone()).expect("Expr rehydrates");
    assert!(
        matches!(expr, crate::ast::Expr::Like { .. }),
        "the refinement is a LIKE predicate, got {expr:?}"
    );
}

#[test]
fn create_type_name_is_a_bare_qualified_name_canonicalized() {
    // §5.5 — a TYPE is DEFINED by a bare, possibly `/`-qualified NAME (never a `/type/…` path). The
    // surface is the name; the stored catalog `name` canonicalizes back to `/type/<segments>`.
    let e = effect_of(parse_ok("CREATE TYPE email (value text)"));
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("type"));
    assert_eq!(values_cell(&e, "name").as_deref(), Some("/type/email"));

    // A qualified name (`chatwork/message`) joins its segments under `/type/`.
    let e = effect_of(parse_ok(
        "CREATE TYPE chatwork/message (message_id text PRIMARY KEY)",
    ));
    assert_eq!(
        values_cell(&e, "name").as_deref(),
        Some("/type/chatwork/message")
    );
}

#[test]
fn create_type_column_type_surface_preserves_canonical_tokens_and_declared_names() {
    let e = effect_of(parse_ok(
        "CREATE TYPE customer (\
         id int PRIMARY KEY, \
         email email, \
         message chatwork/message, \
         document json, \
         payload bytes)",
    ));
    let body = values_cell(&e, "body").expect("object body");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("object body");
    let columns = parsed
        .get("columns")
        .and_then(|c| c.as_array())
        .expect("columns array");
    let ty = |idx: usize| {
        columns[idx]
            .get("type")
            .and_then(|v| v.as_str())
            .expect("column type")
    };
    assert_eq!(ty(0), "int");
    assert_eq!(ty(1), "/type/email");
    assert_eq!(ty(2), "/type/chatwork/message");
    assert_eq!(ty(3), "json");
    assert_eq!(ty(4), "bytes");
}

#[test]
fn create_type_rejects_the_legacy_type_path_form() {
    // The retired `/type/…` PATH form (a definition addressed as data) no longer parses — paths are
    // data, names are definitions. The leading `/` lexes as a `Token::Path`, so `type_name` fails.
    let _ = parse_err("CREATE TYPE /type/email (value text)");
    let _ = parse_err("CREATE TYPE /type/chatwork/message (id text)");
}

#[test]
fn declared_view_of_reference_rejects_a_type_path() {
    // The `OF <name>` reference points at a DEFINITION, so a `/type/…` path there is a category
    // error and is rejected (whereas the view's OWN path stays a path).
    let _ = parse_err(
        "CREATE VIEW /chatwork/rooms OF /type/chatwork/message AS /http/chatwork/rooms |> DECODE json",
    );
}

#[test]
fn declared_view_with_path_name_desugars() {
    let e = effect_of(parse_ok(
        "CREATE VIEW /chatwork/rooms AS /http/chatwork/rooms |> DECODE json",
    ));
    assert_eq!(target_path(&e), "/sys/drivers");
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("view"));
    assert_eq!(values_cell(&e, "name").as_deref(), Some("/chatwork/rooms"));
    assert_eq!(values_cell(&e, "of_type"), None);
    // The body is stored as the pipeline's serde JSON (re-hydratable at eval time).
    assert!(values_cell(&e, "body").unwrap().contains("http"));
}

#[test]
fn declared_view_with_param_and_of_type() {
    let e = effect_of(parse_ok(
        "CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS \
         /http/chatwork/rooms/{room}/messages |> DECODE json",
    ));
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("view"));
    // The `{room}` template segment is preserved verbatim in the node path.
    assert_eq!(
        values_cell(&e, "name").as_deref(),
        Some("/chatwork/rooms/{room}/messages")
    );
    assert_eq!(
        values_cell(&e, "of_type").as_deref(),
        Some("/type/chatwork/message")
    );
}

#[test]
fn create_map_desugars_with_verb_and_body() {
    // NOTE (§13 park): the blueprint writes the map body as `VALUES (ENCODE json)`, but `ENCODE`
    // is a pipe op, not a value expression, so that literal shorthand does not parse. The codec is
    // instead the driver's `default_codec` (json); the map body carries the wire target + verb.
    let e = effect_of(parse_ok(
        "CREATE MAP INSERT /chatwork/rooms/{room}/messages AS \
         INSERT INTO /http/chatwork/rooms/{room}/messages VALUES (row)",
    ));
    assert_eq!(target_path(&e), "/sys/drivers");
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("map"));
    assert_eq!(values_cell(&e, "verb").as_deref(), Some("INSERT"));
    assert_eq!(
        values_cell(&e, "name").as_deref(),
        Some("/chatwork/rooms/{room}/messages")
    );
    assert!(!values_bool(&e, "irreversible"));
    assert!(values_cell(&e, "body").unwrap().contains("http"));
}

#[test]
fn create_map_call_signature_and_irreversible() {
    let e = effect_of(parse_ok(
        "CREATE MAP CALL github.merge /github/prs/{id} AS \
         INSERT INTO /http/github/pulls/{id}/merge VALUES (row) IRREVERSIBLE",
    ));
    assert_eq!(
        values_cell(&e, "verb").as_deref(),
        Some("CALL github.merge")
    );
    assert!(values_bool(&e, "irreversible"));
}

#[test]
fn create_sql_resource_desugars_with_dialect_endpoint_and_table_catalog() {
    // ticket 20260718203326: the declared sql-resource arm — a sqlite-dialect SQL endpoint over a
    // wire query verb, with the relation catalog declared INLINE. Desugars to a `kind='sql'`
    // /sys/drivers row whose `body` carries the dialect, the `/http/<driver>/…` query endpoint, and
    // the table catalog (the declared twin of a mount-time D1 introspection).
    let e = effect_of(parse_ok(
        "CREATE SQL /cloudflare/d1/{database} \
         OVER /http/cloudflare/accounts/{account}/d1/database/{database}/query \
         TABLES ( users ( id text PRIMARY KEY, email text NOT NULL, name text ), \
                  orders ( id text PRIMARY KEY, total int ) )",
    ));
    assert_eq!(target_path(&e), "/sys/drivers");
    assert_eq!(values_cell(&e, "kind").as_deref(), Some("sql"));
    assert_eq!(
        values_cell(&e, "name").as_deref(),
        Some("/cloudflare/d1/{database}")
    );
    let body = values_cell(&e, "body").unwrap();
    assert!(body.contains("\"dialect\":\"sqlite\""), "body: {body}");
    assert!(
        body.contains("/http/cloudflare/accounts/{account}/d1/database/{database}/query"),
        "body carries the query endpoint: {body}"
    );
    assert!(body.contains("users") && body.contains("orders") && body.contains("email"));
    // No secret and no irreversible flag: a declared sql-resource is credential-free by construction.
    assert!(!values_bool(&e, "irreversible"));
}

#[test]
fn create_sql_resource_defaults_dialect_and_rejects_a_non_sqlite_dialect() {
    // DIALECT is optional (default sqlite). Only sqlite is served today (the driver-cf planner
    // dialect); any other dialect is a crisp parse error, never a silent mismatch.
    assert!(
        parse_statement("CREATE SQL /cf/d1/{db} OVER /http/cf/query TABLES ( t ( id text ) )")
            .is_ok()
    );
    assert!(parse_statement(
        "CREATE SQL /cf/d1/{db} DIALECT SQLITE OVER /http/cf/query TABLES ( t ( id text ) )"
    )
    .is_ok());
    assert!(parse_statement(
        "CREATE SQL /cf/d1/{db} DIALECT postgres OVER /http/cf/query TABLES ( t ( id text ) )"
    )
    .is_err());
}

#[test]
fn server_binding_view_with_bare_ident_still_parses_as_ddl() {
    // Dispatch regression + PRINCIPLE (§5.5): the two `CREATE VIEW` forms are told apart by what KIND
    // of thing the view's OWN name denotes. A PATH name (`CREATE VIEW /chatwork/rooms`) is a
    // *readable data surface* — a mount other queries read `FROM` — so it is the declared view. A
    // BARE name (`CREATE VIEW recent`) is a *server binding*, an operator handle that is not itself a
    // path, so the declared-view arm backtracks and `server_ddl` claims it as a `Statement::Ddl`.
    // This is the same paths=data / names=definitions cut the `OF <name>` reference makes.
    let stmt = parse_ok("CREATE VIEW recent AS /mail/inbox");
    assert!(
        matches!(stmt, Statement::Ddl(ref d) if d.kind == DdlKind::View),
        "bare-ident CREATE VIEW must remain a server-binding DDL, got {stmt:?}"
    );
    // And the other server-DDL forms are unaffected.
    assert!(matches!(
        parse_ok("CREATE MATERIALIZED VIEW mv AS /t"),
        Statement::Ddl(_)
    ));
    assert!(matches!(
        parse_ok("CREATE ENDPOINT recent ON 'GET /recent' AS /mail/inbox"),
        Statement::Ddl(_)
    ));
}

#[test]
fn declared_driver_nouns_add_no_frozen_keyword() {
    // The additive-by-contextual-ident contract: every §13 noun is a `word(...)` ident, never a
    // frozen keyword (the keyword-freeze lock stays unchanged).
    for w in [
        "driver",
        "type",
        "map",
        "auth",
        "bearer",
        "header",
        "oauth2",
        "paginate",
        "cursor",
        "link",
        "of",
        "irreversible",
        "authorize",
        "token",
        "scopes",
        "next",
        "param",
        "max",
        "sql",
        "over",
        "tables",
        "dialect",
    ] {
        assert!(
            !KEYWORDS.contains(&w),
            "`{w}` must NOT be a frozen keyword (it is a §13 contextual ident)"
        );
    }
}

#[test]
fn template_path_rejects_a_glob_or_version_collision() {
    // A `{param}` segment must be a clean `{name}` — it cannot also be a glob or an `@version`, so
    // the template seam never overlaps the existing path coordinates.
    assert!(parse_statement("CREATE VIEW /a/{b*} AS /http/x").is_err());
    assert!(
        parse_statement("CREATE MAP INSERT /a/{b}@v1 AS INSERT INTO /http/x VALUES (1)").is_err()
    );
    // A clean template segment is fine.
    assert!(parse_statement("CREATE VIEW /a/{b}/c AS /http/x |> DECODE json").is_ok());
}

#[test]
fn follow_stage_parses_as_the_follow_pipe_op() {
    // FOLLOW (blueprint §13, ticket 20260711121526): a contextual-identifier stage naming the
    // delivered field whose text value is the second GET's URL — the declared file-download shape.
    let Statement::Query(p) =
        parse_ok("/http/chatwork/rooms/1/files/9 |> DECODE json |> FOLLOW download_url")
    else {
        panic!("expected a query");
    };
    match p.ops.as_slice() {
        [PipeOp::Decode(_), PipeOp::Follow(f)] => assert_eq!(f.field, "download_url"),
        other => panic!("expected DECODE then FOLLOW, got {other:?}"),
    }
}

#[test]
fn insert_with_encode_stage_desugars_to_a_values_pipeline_body() {
    // ENCODE-between-target-and-VALUES (the §13 declared upload shape, ticket 20260711121526)
    // desugars onto the EXISTING EffectBody::Pipeline shape — a VALUES source with exactly one
    // ENCODE stage — so the closed statement/AST shapes are untouched.
    let Statement::Effect(e) =
        parse_ok("INSERT INTO /http/chatwork/rooms/1/files |> ENCODE multipart VALUES (row)")
    else {
        panic!("expected an effect");
    };
    let EffectBody::Pipeline(p) = &e.body else {
        panic!("expected the desugared pipeline body, got {:?}", e.body);
    };
    assert!(matches!(&p.source, Source::Values(v) if v.rows.len() == 1));
    match p.ops.as_slice() {
        [PipeOp::Encode(c)] => assert_eq!(c.fmt, "multipart"),
        other => panic!("expected exactly one ENCODE stage, got {other:?}"),
    }
}

#[test]
fn full_chatwork_script_parses_statement_for_statement() {
    // Blueprint §13's fenced example: every statement parses and desugars to a `/sys/drivers` row.
    for src in [
        "CREATE DRIVER chatwork AT 'https://api.chatwork.com/v2' AUTH HEADER 'x-chatworktoken'",
        "CREATE TYPE chatwork/message (message_id text PRIMARY KEY, body text NOT NULL, send_time timestamp)",
        "CREATE VIEW /chatwork/rooms AS /http/chatwork/rooms |> DECODE json",
        "CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS /http/chatwork/rooms/{room}/messages |> DECODE json",
        // §13 park: `VALUES (ENCODE json)` is blueprint shorthand that does not parse (ENCODE is a
        // pipe op); the codec is the driver default and the body carries the wire target.
        "CREATE MAP INSERT /chatwork/rooms/{room}/messages AS INSERT INTO /http/chatwork/rooms/{room}/messages VALUES (row)",
    ] {
        let e = effect_of(parse_ok(src));
        assert_eq!(target_path(&e), "/sys/drivers", "desugars to /sys/drivers: {src}");
    }
}

#[test]
fn connect_account_and_host_carry_the_mount_coordinate() {
    // ADR 0008 §4: the mount carries the (host, driver, account) coordinate. ACCOUNT/HOST are
    // clause-loop contextual idents like AT/SECRET, collected in any order.
    let e = effect_of(parse_ok(
        "CONNECT /mail TO gmail ACCOUNT 'you@work.example' HOST 'local'",
    ));
    assert_eq!(values_cell(&e, "driver").as_deref(), Some("gmail"));
    assert_eq!(
        values_cell(&e, "account").as_deref(),
        Some("you@work.example")
    );
    assert_eq!(values_cell(&e, "host").as_deref(), Some("local"));
    // Order-independent with the other clauses; absent clauses stay NULL (host resolves to the
    // implicit `local` at the applier, not in the desugar).
    let e = effect_of(parse_ok(
        "CONNECT /m TO gmail SECRET 'vault:gmail/work' ACCOUNT 'me@personal.example'",
    ));
    assert_eq!(
        values_cell(&e, "account").as_deref(),
        Some("me@personal.example")
    );
    assert_eq!(values_cell(&e, "host"), None);
    // An account-less CONNECT (a local source) carries NULLs — the pre-ADR shape still parses.
    let e = effect_of(parse_ok("CONNECT /db TO sqlite AT '/data/l.db'"));
    assert_eq!(values_cell(&e, "account"), None);
    assert_eq!(values_cell(&e, "host"), None);
}

#[test]
fn account_and_host_stay_ordinary_identifiers() {
    // The contextual-ident regression: `account` / `host` remain usable as plain column names in a
    // query — adding the CONNECT clauses must not steal them from the identifier space.
    assert!(parse_statement("/sql/db/users |> select account, host |> limit 5").is_ok());
    assert!(parse_statement("/sql/db/users |> where account == 'x' AND host == 'y'").is_ok());
}

#[test]
fn ddl_trigger_do_plan() {
    // The DO clause holds an effect-plan (a statement). A trigger that archives a
    // matched row on an inbox event:
    let stmt = parse_ok("CREATE TRIGGER notify ON inbox DO INSERT INTO /log VALUES ('fired')");
    let Statement::Ddl(d) = stmt else { panic!() };
    assert_eq!(d.kind, DdlKind::Trigger);
    assert_eq!(d.on.as_deref(), Some("inbox"));
    // DO clause holds an inner statement.
    assert!(d.do_plan.is_some());
    assert!(matches!(d.do_plan.as_deref(), Some(Statement::Effect(_))));
}

#[test]
fn ddl_trigger_with_where_guard() {
    // t34 (CO-t31-4): `CREATE TRIGGER … ON <event> WHERE <pred> DO <plan>` — the optional WHERE
    // guard is captured in `where_pred` (a Query wrapping the predicate over an empty VALUES
    // source), distinct from the DO body's own statement.
    let stmt = parse_ok(
        "CREATE TRIGGER notify ON inbox WHERE priority > 3 DO INSERT INTO /log VALUES ('fired')",
    );
    let Statement::Ddl(d) = stmt else { panic!() };
    assert_eq!(d.kind, DdlKind::Trigger);
    assert_eq!(d.on.as_deref(), Some("inbox"));
    // The WHERE guard is surfaced as a Query whose single op is the predicate.
    let Some(Statement::Query(p)) = d.where_pred.as_deref() else {
        panic!("expected where_pred query")
    };
    assert_eq!(p.ops.len(), 1);
    assert!(matches!(p.ops[0], PipeOp::Where(_)));
    // The DO body is still the effect statement (its own clause, not the guard).
    assert!(matches!(d.do_plan.as_deref(), Some(Statement::Effect(_))));
}

#[test]
fn ddl_job_every() {
    let stmt = parse_ok("CREATE JOB nightly EVERY '1h' DO REMOVE /tmp WHERE age > 7");
    let Statement::Ddl(d) = stmt else { panic!() };
    assert_eq!(d.kind, DdlKind::Job);
    assert_eq!(d.every.as_deref(), Some("1h"));
    assert!(d.do_plan.is_some());
}

#[test]
fn ddl_view_and_materialized_view() {
    let v = parse_ok("CREATE VIEW recent AS /mail/inbox");
    let Statement::Ddl(d) = v else { panic!() };
    assert_eq!(d.kind, DdlKind::View);

    let mv = parse_ok("CREATE MATERIALIZED VIEW cached AS /mail/inbox");
    let Statement::Ddl(d) = mv else { panic!() };
    assert_eq!(d.kind, DdlKind::MaterializedView);
    assert_eq!(d.target, vec!["server", "materialized_views", "cached"]);
}

#[test]
fn ddl_webhook_and_policy() {
    let w = parse_ok("CREATE WEBHOOK inbound ON '/hooks/x'");
    let Statement::Ddl(d) = w else { panic!() };
    assert_eq!(d.kind, DdlKind::Webhook);

    let pol = parse_ok("CREATE POLICY leastpriv");
    let Statement::Ddl(d) = pol else { panic!() };
    assert_eq!(d.kind, DdlKind::Policy);
    assert!(d.policy_rules.is_empty());
}

#[test]
fn ddl_agent_parses_and_adds_no_frozen_keyword() {
    // blueprint §19: `CREATE AGENT <name>` — `AGENT` is a contextual UPPERCASE ident (like
    // CONNECTION/MATERIALIZED), so it adds NO frozen keyword.
    let stmt = parse_ok("CREATE AGENT triage");
    let Statement::Ddl(d) = stmt else { panic!() };
    assert_eq!(d.kind, DdlKind::Agent);
    assert_eq!(d.name, "triage");
    assert_eq!(d.target, vec!["server", "agents", "triage"]);

    // An attached POLICY handle rides the frozen `POLICY` keyword (the name is a bare ident).
    let stmt = parse_ok("CREATE AGENT triage POLICY p");
    let Statement::Ddl(d) = stmt else { panic!() };
    assert_eq!(d.kind, DdlKind::Agent);
    assert_eq!(d.policy.as_deref(), Some("p"));

    // `agent` is NOT a frozen keyword — a column named `agent` still parses everywhere (the
    // 39-keyword freeze lock stays intact).
    assert!(!KEYWORDS.contains(&"agent"));
    assert!(parse_statement("/x |> SELECT agent").is_ok());
    assert!(parse_statement("/x |> WHERE agent > 3").is_ok());
}

#[test]
fn ddl_policy_allow_deny_rules() {
    // The blueprint §10 example: `ALLOW SELECT DENY INSERT,UPDATE,REMOVE,CALL`. `ALLOW`/`DENY` are
    // contextual idents (NOT frozen keywords); verbs span keyword (SELECT/UPDATE/REMOVE/CALL)
    // and ident (INSERT/UPSERT) lexer shapes.
    let p = parse_ok("CREATE POLICY api ALLOW SELECT DENY INSERT,UPDATE,REMOVE,CALL");
    let Statement::Ddl(d) = p else { panic!() };
    assert_eq!(d.kind, DdlKind::Policy);
    assert_eq!(d.policy_rules.len(), 2);
    assert!(d.policy_rules[0].allow);
    assert_eq!(d.policy_rules[0].verbs, vec!["SELECT"]);
    assert!(!d.policy_rules[0].all_token);
    assert!(!d.policy_rules[1].allow);
    assert_eq!(
        d.policy_rules[1].verbs,
        vec!["INSERT", "UPDATE", "REMOVE", "CALL"]
    );

    // `ALLOW ALL ON mail` — bare ALL token + a per-rule driver glob.
    let p = parse_ok("CREATE POLICY broad ALLOW ALL ON mail");
    let Statement::Ddl(d) = p else { panic!() };
    assert_eq!(d.policy_rules.len(), 1);
    assert!(d.policy_rules[0].all_token);
    assert_eq!(d.policy_rules[0].verbs, vec!["ALL"]);
    assert_eq!(d.policy_rules[0].driver.as_deref(), Some("mail"));

    // `ALLOW UPSERT ON 's3/*'` — INSERT/UPSERT bare-ident verbs + a quoted glob (a glob with
    // `/` or `*` must be quoted, like the `ON 'GET /route'` endpoint operand).
    let p = parse_ok("CREATE POLICY s3w ALLOW UPSERT ON 's3/*'");
    let Statement::Ddl(d) = p else { panic!() };
    assert_eq!(d.policy_rules[0].verbs, vec!["UPSERT"]);
    assert_eq!(d.policy_rules[0].driver.as_deref(), Some("s3/*"));
}

#[test]
fn ddl_policy_t57_actor_scope_and_condition_clauses() {
    // t57: `FOR <kind> <name>` (actor), `AT <path>` (realm-scoped path), and `WHERE <expr>`
    // (conditional grant via the ORDINARY `member_of(...)` call) all parse onto a policy rule
    // WITHOUT adding a keyword — `FOR`/`AT`/`user`/`role`/`group` are contextual idents and
    // `WHERE` is already frozen; `member_of(...)` is an `Expr::Fn`.
    let p = parse_ok(
        "CREATE POLICY eng ALLOW INSERT ON mail FOR role admin AT /members/alice/** \
         WHERE member_of('/directories/google/groups/eng')",
    );
    let Statement::Ddl(d) = p else { panic!() };
    assert_eq!(d.policy_rules.len(), 1);
    let r = &d.policy_rules[0];
    assert!(r.allow);
    assert_eq!(r.verbs, vec!["INSERT"]);
    assert_eq!(r.driver.as_deref(), Some("mail"));
    let subject = r.subject.as_ref().expect("a FOR clause");
    assert_eq!(subject.kind, "role");
    assert_eq!(subject.name, "admin");
    assert_eq!(r.scope.as_deref(), Some("/members/alice/**"));
    // The WHERE body is an ordinary function call (the "functions are values" seam).
    match r.condition.as_ref().expect("a WHERE clause") {
        Expr::Fn(call) => {
            assert_eq!(call.name, "member_of");
            assert_eq!(call.args.len(), 1);
        }
        other => panic!("expected a member_of(...) call, got {other:?}"),
    }

    // blueprint §19 axis B: `FOR agent <name>` parses as a policy subject beside user/role/group,
    // adding no frozen keyword (`agent` is a contextual ident).
    let p = parse_ok("CREATE POLICY ag ALLOW INSERT ON mail FOR agent triage AT /me/mail/**");
    let Statement::Ddl(d) = p else { panic!() };
    let r = &d.policy_rules[0];
    let subject = r.subject.as_ref().expect("a FOR agent clause");
    assert_eq!(subject.kind, "agent");
    assert_eq!(subject.name, "triage");
    assert_eq!(r.scope.as_deref(), Some("/me/mail/**"));

    // The clauses are optional: a bare ALLOW rule has none of them.
    let p = parse_ok("CREATE POLICY plain ALLOW SELECT");
    let Statement::Ddl(d) = p else { panic!() };
    assert!(d.policy_rules[0].subject.is_none());
    assert!(d.policy_rules[0].scope.is_none());
    assert!(d.policy_rules[0].condition.is_none());
}

// ---- plan wrappers --------------------------------------------------------

#[test]
fn preview_and_commit_wrappers() {
    let pv = parse_ok("PREVIEW REMOVE /mail/spam WHERE age > 30");
    let Statement::Plan(p) = pv else {
        panic!("expected Plan")
    };
    assert!(!p.commit);
    assert!(matches!(*p.inner, Statement::Effect(_)));

    let cm = parse_ok("COMMIT INSERT INTO /t VALUES (1)");
    let Statement::Plan(p) = cm else { panic!() };
    assert!(p.commit);
}

// ---- governance: closed-core variant counts ------------------------------

/// The closed-core thesis (blueprint §3): the `Statement` and `PipeOp` variant sets are
/// fixed. This test mechanically locks their counts so a later ticket cannot smuggle
/// a per-driver / per-action variant into the grammar. Adding a variant here is a
/// deliberate, reviewed change-control event.
#[test]
fn closed_core_variant_counts_are_locked() {
    // 6 statement forms (Query, Effect, Ddl, Plan, Let, Transaction). `Let` (t60) and
    // `Transaction` (t62) are the deliberate M6 functional-core additions, each gated by exactly
    // this lock + the keyword freeze so the addition is reviewed, not smuggled in.
    let statement_variants = [
        Statement::Query(Pipeline {
            source: Source::Values(Values {
                columns: None,
                rows: vec![],
            }),
            ops: vec![],
        }),
        Statement::Let {
            name: String::new(),
            value: Box::new(Statement::Query(Pipeline {
                source: Source::Values(Values {
                    columns: None,
                    rows: vec![],
                }),
                ops: vec![],
            })),
            body: Box::new(Statement::Query(Pipeline {
                source: Source::Values(Values {
                    columns: None,
                    rows: vec![],
                }),
                ops: vec![],
            })),
        },
        Statement::Effect(EffectStmt {
            verb: EffectVerb::Insert,
            target: PathExpr {
                segments: vec![],
                as_of: None,
                span: Span::new(0, 0),
            },
            body: EffectBody::SetWhere {
                set: vec![],
                filter: None,
            },
            returning: None,
        }),
        Statement::Ddl(ServerDdl {
            kind: DdlKind::Policy,
            name: String::new(),
            target: vec![],
            do_plan: None,
            as_query: None,
            where_pred: None,
            every: None,
            on: None,
            policy_rules: vec![],
            policy: None,
            connection: None,
        }),
        Statement::Plan(PlanWrap {
            commit: false,
            inner: Box::new(Statement::Query(Pipeline {
                source: Source::Values(Values {
                    columns: None,
                    rows: vec![],
                }),
                ops: vec![],
            })),
            span: Span::new(0, 0),
        }),
        Statement::Transaction {
            body: vec![],
            span: Span::new(0, 0),
        },
    ];
    assert_eq!(
        statement_variants.len(),
        6,
        "Statement is closed at 6 forms (blueprint §3 + the t60 `LET` and t62 `TRANSACTION` \
         functional-core additions)"
    );

    // 18 closed-core pipe ops — each maps to a frozen query/transform keyword or a
    // registry seam (Decode/Encode/Call). There is NO per-action variant.
    let pipe_variants = [
        PipeOp::Where(Expr::Lit(Literal::Null)),
        PipeOp::Select(vec![]),
        PipeOp::Extend(vec![]),
        PipeOp::Set(vec![]),
        PipeOp::Aggregate(vec![]),
        PipeOp::GroupBy(vec![]),
        PipeOp::OrderBy(vec![]),
        PipeOp::Limit(0),
        PipeOp::Distinct,
        PipeOp::Join(JoinOp {
            source: Source::Values(Values {
                columns: None,
                rows: vec![],
            }),
            on: Expr::Lit(Literal::Null),
        }),
        PipeOp::Union(Box::new(Pipeline {
            source: Source::Values(Values {
                columns: None,
                rows: vec![],
            }),
            ops: vec![],
        })),
        PipeOp::Except(Box::new(Pipeline {
            source: Source::Values(Values {
                columns: None,
                rows: vec![],
            }),
            ops: vec![],
        })),
        PipeOp::Intersect(Box::new(Pipeline {
            source: Source::Values(Values {
                columns: None,
                rows: vec![],
            }),
            ops: vec![],
        })),
        PipeOp::As(String::new()),
        PipeOp::Expand(vec![]),
        PipeOp::Decode(Codec {
            fmt: String::new(),
            span: Span::new(0, 0),
        }),
        PipeOp::Encode(Codec {
            fmt: String::new(),
            span: Span::new(0, 0),
        }),
        PipeOp::Call(CallRef {
            driver: String::new(),
            action: String::new(),
            args: vec![],
            span: Span::new(0, 0),
        }),
        // TRANSFORM (blueprint §15, decision W) — the model-calling stage, a deliberate,
        // reviewed additive variant (18 → 19). `transform` is a CONTEXTUAL identifier, not a
        // frozen keyword, so the keyword freeze stays at 39; only this variant set grew.
        PipeOp::Transform(TransformRef {
            name: String::new(),
            span: Span::new(0, 0),
        }),
        // SWITCH (blueprint §18) — the model-routing stage, the second deliberate, reviewed
        // additive variant (19 → 20). `switch`/`else` are CONTEXTUAL identifiers, not frozen
        // keywords, so the keyword freeze stays at 39; only this variant set grew.
        PipeOp::Switch(SwitchStage {
            discriminant: String::new(),
            arms: vec![],
            span: Span::new(0, 0),
        }),
        // FOLLOW (blueprint §13, ticket 20260711121526) — the declared-driver second-fetch
        // stage, the third deliberate, reviewed additive variant (20 → 21). `follow` is a
        // CONTEXTUAL identifier, not a frozen keyword, so the keyword freeze stays at 39; only
        // this variant set grew. Outside a declared view body it is a structured lowering/eval
        // refusal, never an executable general-pipeline stage.
        PipeOp::Follow(FollowRef {
            field: String::new(),
            span: Span::new(0, 0),
        }),
        // OF (blueprint §5.6, ticket 20260714154144) — the general use-site type-assertion stage,
        // the fourth deliberate, reviewed additive variant (21 → 22). It satisfies admission
        // criterion (2) of §5.3a (it asserts/names the relation type — a plan-time schema check,
        // schema-identity at runtime, no effect). `of` is a CONTEXTUAL identifier, not a frozen
        // keyword (it is already `word("OF")` in the DDL), so the keyword freeze stays at 39; only
        // this variant set grew.
        PipeOp::Of(OfRef {
            target: OfTarget::Named(String::new()),
            span: Span::new(0, 0),
        }),
    ];
    assert_eq!(
        pipe_variants.len(),
        22,
        "PipeOp is closed at 22 variants — the §15 TRANSFORM stage (decision W), the §18 \
         SWITCH stage, the §13 FOLLOW stage, and the §5.6 OF assertion stage are the only \
         additive growth; still no per-driver/per-action variant (blueprint §3)"
    );
}

// ---- LET binding + multi-statement program (M6, ticket t60) ---------------

#[test]
fn let_binding_binds_a_relation_and_threads_body() {
    // A single LET binding followed by the statement that uses it (one per line, `;`-free).
    let stmt = parse_ok(
        "LET active = /sql/pg/customers |> WHERE status == 'active'\n\
         active |> SELECT id",
    );
    let Statement::Let { name, value, body } = stmt else {
        panic!("expected a LET binding, got {stmt:?}")
    };
    assert_eq!(name, "active");
    // The bound value is always a relation (a Query pipeline), never an effect.
    let Statement::Query(v) = *value else {
        panic!("LET value must be a Query")
    };
    let Source::Path(p) = v.source else {
        panic!("expected a path source")
    };
    assert_eq!(p.segments[0].name, "sql");
    // The body references the bound name as a bare-identifier source.
    let Statement::Query(b) = *body else {
        panic!("LET body must be the using statement")
    };
    assert_eq!(b.source, Source::Name("active".to_string()));
}

#[test]
fn multiple_let_bindings_nest_left_to_right() {
    // Two `;`-free LET lines stay in scope for the final statement (lexical, nested).
    let stmt = parse_ok(
        "LET a = /sql/pg/x\n\
         LET b = /sql/pg/y\n\
         a |> UNION b",
    );
    let Statement::Let { name, body, .. } = stmt else {
        panic!("expected outer LET a")
    };
    assert_eq!(name, "a");
    let Statement::Let {
        name: inner,
        body: inner_body,
        ..
    } = *body
    else {
        panic!("expected nested LET b")
    };
    assert_eq!(inner, "b");
    let Statement::Query(q) = *inner_body else {
        panic!("expected the final query")
    };
    assert_eq!(q.source, Source::Name("a".to_string()));
}

#[test]
fn bare_identifier_is_a_valid_source() {
    // `products` (a bare name, no leading slash, no `FROM`) is a bound-name source — shape-only
    // at parse time (decision R, t73: the source leads).
    let stmt = parse_ok("products |> LIMIT 5");
    let Statement::Query(p) = stmt else {
        panic!("expected Query")
    };
    assert_eq!(p.source, Source::Name("products".to_string()));
}

#[test]
fn let_value_may_not_be_an_effect() {
    // A LET binds a relation only; an effect value is a crisp parse error (`cut_err` after
    // the `=`), never a smuggled write into a pure context.
    let err = parse_err("LET x = INSERT INTO /sql/pg/t VALUES (1)\nx");
    assert!(!err.expected.is_empty());
}

#[test]
fn lowercase_let_parses_as_a_binding() {
    // Case policy (t74, decision S): keywords are lowercase canonical and recognized
    // case-insensitively. The lowercase `let` IS the binding keyword now — it parses, and
    // so does the historical uppercase `LET` (both fold to the same keyword).
    assert!(matches!(parse_ok("let x = /a\nx"), Statement::Let { .. }));
    assert!(matches!(parse_ok("LET x = /a\nx"), Statement::Let { .. }));
    assert!(matches!(parse_ok("Let x = /a\nx"), Statement::Let { .. }));
}

#[test]
fn two_bare_statements_without_a_binding_are_rejected() {
    // The program model is `LET* <final-statement>`; two non-binding statements in a row is
    // trailing input, not a silent split.
    let err = parse_err("/sql/pg/a\n/sql/pg/b");
    assert!(!err.expected.is_empty());
}

// ---- lambdas as values + higher-order fns (M6, ticket t61) ----------------

/// Pull the single `WHERE` predicate expression out of a one-stage query (the full
/// expression grammar — including lambdas — is reachable there).
fn where_expr(src: &str) -> Expr {
    let Statement::Query(p) = parse_ok(src) else {
        panic!("expected a query")
    };
    let PipeOp::Where(e) = p.ops.into_iter().next().expect("a WHERE op") else {
        panic!("expected a WHERE op")
    };
    e
}

#[test]
fn lambda_literal_with_type_annotation_parses() {
    // The roadmap §1.2 canonical lambda: `(addr: text) => lower(trim(addr))`. NO keyword is
    // added — the form rides the expression grammar and reuses the `=>` token.
    let Expr::Lambda { params, body } = where_expr("/t |> WHERE (addr: text) => lower(trim(addr))")
    else {
        panic!("expected a lambda literal")
    };
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "addr");
    // The annotation is parsed-and-retained (lowercase primitive per decision S/T).
    assert_eq!(params[0].ty.as_ref().map(|t| t.name.as_str()), Some("text"));
    // The body is the nested function call `lower(trim(addr))`.
    assert!(matches!(*body, Expr::Fn(_)));
}

#[test]
fn lambda_type_annotation_parses_recursive_column_types_and_resource() {
    let Expr::Lambda { params, .. } =
        where_expr("/t |> WHERE (xs: array<struct<id:int,label:text>>) => xs")
    else {
        panic!("expected a lambda literal")
    };
    assert_eq!(
        params[0].ty.as_ref().map(|t| t.name.as_str()),
        Some("array<struct<id:int,label:text>>")
    );

    let Expr::Lambda { params, .. } = where_expr("/t |> WHERE (r: Resource) => r") else {
        panic!("expected a lambda literal")
    };
    assert_eq!(
        params[0].ty.as_ref().map(|t| t.name.as_str()),
        Some("Resource")
    );
}

#[test]
fn bare_and_multi_parameter_lambdas_parse() {
    // A bare parameter (no annotation) retains `ty: None`.
    let Expr::Lambda { params, .. } = where_expr("/t |> WHERE (x) => x") else {
        panic!("expected a lambda")
    };
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].ty, None);

    // Multiple parameters, mixed annotated / bare.
    let Expr::Lambda { params, .. } = where_expr("/t |> WHERE (acc: int, item) => acc == item")
    else {
        panic!("expected a 2-param lambda")
    };
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "acc");
    assert_eq!(params[0].ty.as_ref().map(|t| t.name.as_str()), Some("int"));
    assert_eq!(params[1].name, "item");
    assert_eq!(params[1].ty, None);
}

#[test]
fn lambda_as_a_function_argument_parses() {
    // A lambda flows into `map(col, fn)` as just another argument expression — no new call
    // machinery (the `FnRef` already carries `args: Vec<Expr>`).
    let Expr::Fn(f) = where_expr("/t |> WHERE map(tags, (t: text) => upper(t))") else {
        panic!("expected a fn call")
    };
    assert_eq!(f.name, "map");
    assert_eq!(f.args.len(), 2);
    assert!(matches!(f.args[0], Expr::Col(_)));
    assert!(matches!(f.args[1], Expr::Lambda { .. }));
}

#[test]
fn parenthesised_expression_is_not_a_lambda() {
    // The lambda parser backtracks cleanly when a `( … )` group is not followed by `=>`, so a
    // plain parenthesised sub-expression still parses as itself.
    let e = where_expr("/t |> WHERE (a == b)");
    assert!(matches!(e, Expr::Binary { op: Op::Eq, .. }));
}

#[test]
fn arithmetic_operators_parse_with_precedence() {
    let e = where_expr("/t |> WHERE price * qty + tax > total");
    let Expr::Binary {
        op: Op::Gt,
        lhs,
        rhs,
    } = e
    else {
        panic!("expected comparison over arithmetic expression");
    };
    assert!(matches!(*rhs, Expr::Col(ref c) if c == "total"));
    let Expr::Binary {
        op: Op::Add,
        lhs: add_lhs,
        rhs: add_rhs,
    } = *lhs
    else {
        panic!("expected additive expression on comparison lhs");
    };
    assert!(matches!(*add_rhs, Expr::Col(ref c) if c == "tax"));
    assert!(matches!(*add_lhs, Expr::Binary { op: Op::Mul, .. }));
}

#[test]
fn slash_after_operand_is_division_not_a_path() {
    let e = where_expr("/t |> WHERE total / count > 3.0");
    let Expr::Binary {
        op: Op::Gt, lhs, ..
    } = e
    else {
        panic!("expected comparison");
    };
    assert!(matches!(*lhs, Expr::Binary { op: Op::Div, .. }));
}

#[test]
fn slash_after_path_boundary_word_stays_a_path() {
    let Statement::Query(p) = parse_ok("/db/t |> JOIN /contacts ON id == contact_id") else {
        panic!("expected query")
    };
    let PipeOp::Join(join) = &p.ops[0] else {
        panic!("expected JOIN")
    };
    let Source::Path(path) = &join.source else {
        panic!("expected joined path source")
    };
    assert_eq!(path.segments[0].name, "contacts");
}

#[test]
fn let_binds_a_lambda_value_no_def_keyword() {
    // A named function is just a `LET`-bound lambda (no `DEF`): `LET normalize = (addr) => …`.
    // The value binding is retained as a single-cell `VALUES` relation, so `Statement::Let`'s
    // shape (and its governance variant lock) is untouched — no new `Statement` variant.
    let Statement::Let { name, value, .. } = parse_ok(
        "LET normalize = (addr: text) => lower(trim(addr))\n\
         /t |> EXTEND k = normalize(email)",
    ) else {
        panic!("expected a LET binding")
    };
    assert_eq!(name, "normalize");
    let Statement::Query(Pipeline {
        source: Source::Values(v),
        ..
    }) = *value
    else {
        panic!("expected the lambda retained in a VALUES cell")
    };
    assert!(matches!(v.rows[0][0], Expr::Lambda { .. }));
}

#[test]
fn let_binds_a_scalar_value() {
    // A scalar value binding (`LET cutoff = '2026-03-27'`) — also retained as a one-cell VALUES.
    let Statement::Let { name, value, .. } =
        parse_ok("LET cutoff = '2026-03-27'\n/t |> WHERE created_at >= cutoff")
    else {
        panic!("expected a LET binding")
    };
    assert_eq!(name, "cutoff");
    let Statement::Query(Pipeline {
        source: Source::Values(v),
        ..
    }) = *value
    else {
        panic!("expected a one-cell VALUES value binding")
    };
    assert!(matches!(v.rows[0][0], Expr::Lit(Literal::Str(_))));
}

// ---- TRANSACTION block (M6, ticket t62) -----------------------------------

#[test]
fn transaction_block_of_two_upserts_parses() {
    // The roadmap §1.2 canonical shape: two reversible UPSERTs grouped into one block.
    let stmt = parse_ok(
        "TRANSACTION { \
           UPSERT INTO /sql/pg/orders VALUES (1); \
           UPSERT INTO /sql/pg/audit VALUES (1) \
         }",
    );
    let Statement::Transaction { body, .. } = stmt else {
        panic!("expected a TRANSACTION block, got {stmt:?}")
    };
    assert_eq!(body.len(), 2, "two effect members, in source order");
    for member in &body {
        let Statement::Effect(e) = member else {
            panic!("a transaction member must be an Effect, got {member:?}")
        };
        assert_eq!(e.verb, EffectVerb::Upsert);
    }
}

#[test]
fn transaction_block_accepts_trailing_semicolon_and_pipe_stage_writes() {
    // A trailing `;` is allowed; a pipe-stage write (decision Q) is a legal effect member.
    let stmt = parse_ok(
        "TRANSACTION { \
           /sql/pg/src |> WHERE flag == TRUE |> UPDATE SET status = 'done'; \
           INSERT INTO /sql/pg/audit VALUES (1); \
         }",
    );
    let Statement::Transaction { body, .. } = stmt else {
        panic!("expected a TRANSACTION block")
    };
    assert_eq!(body.len(), 2);
    assert!(matches!(body[0], Statement::Effect(_)));
    assert!(matches!(body[1], Statement::Effect(_)));
}

#[test]
fn transaction_block_parses_irreversible_member_shape() {
    // Reversibility is an EVAL-time concern, not a parse-time one: a `REMOVE` inside parses
    // fine here and is rejected later by the evaluator's reversible-only guard.
    let stmt = parse_ok("TRANSACTION { REMOVE /sql/pg/t WHERE id == 1 }");
    let Statement::Transaction { body, .. } = stmt else {
        panic!("expected a TRANSACTION block")
    };
    assert_eq!(body.len(), 1);
    let Statement::Effect(e) = &body[0] else {
        panic!()
    };
    assert_eq!(e.verb, EffectVerb::Remove);
}

#[test]
fn empty_transaction_block_parses() {
    let stmt = parse_ok("TRANSACTION { }");
    let Statement::Transaction { body, .. } = stmt else {
        panic!("expected a TRANSACTION block")
    };
    assert!(body.is_empty());
}

#[test]
fn transaction_block_can_be_wrapped_in_commit() {
    let stmt = parse_ok("COMMIT TRANSACTION { UPSERT INTO /sql/pg/t VALUES (1) }");
    let Statement::Plan(p) = stmt else {
        panic!("expected a plan wrapper")
    };
    assert!(p.commit);
    assert!(matches!(p.inner.as_ref(), Statement::Transaction { .. }));
}

#[test]
fn transaction_body_rejects_a_pure_read() {
    // A read pipeline is not a legal transaction member (reversible-only block holds only
    // effects) — a clear error, never a silent accept.
    let _ = parse_err("TRANSACTION { /sql/pg/t |> LIMIT 5 }");
}

#[test]
fn transaction_does_not_nest() {
    // Conservative this slice (decision G): no nested `TRANSACTION` inside a transaction body.
    let _ = parse_err("TRANSACTION { TRANSACTION { UPSERT INTO /t VALUES (1) } }");
}

// ---- governance: rejection cases ------------------------------------------

#[test]
fn lowercase_keyword_parses_case_insensitively() {
    // Post-t74 (decision S) lowercase keywords ARE the canonical form: `where` parses, and so
    // does `WHERE` / `Where` — all fold to the same stage. (The lone `=` is still the binding
    // token; equivalence is `==`, blueprint decision O / t70 — unrelated to case.)
    for spelling in ["where", "WHERE", "Where"] {
        let Statement::Query(p) = parse_ok(&format!("/mail |> {spelling} id == 1")) else {
            panic!("expected a query for `{spelling}`");
        };
        assert!(matches!(p.ops.first(), Some(PipeOp::Where(_))));
    }
}

#[test]
fn incomplete_multiword_keyword_is_unknown_keyword() {
    // A multi-word keyword fragment standing alone (a `group` with no `by`) is still a crisp
    // `UnknownKeyword` — case-insensitive recognition does not make a fragment valid (t74).
    let err = parse_err("/mail |> group id");
    assert_eq!(err.code, ParseErrorCode::UnknownKeyword);
    assert!(err.span.start > 0);
    assert!(!err.expected.is_empty());
}

#[test]
fn reserved_word_as_identifier_is_rejected() {
    // `SELECT` (a reserved keyword) cannot stand where an identifier (a column) is
    // required.
    let err = parse_err("/t |> SELECT SELECT");
    assert!(
        matches!(
            err.code,
            ParseErrorCode::ReservedAsIdentifier | ParseErrorCode::UnexpectedToken
        ),
        "got {:?}",
        err.code
    );
}

#[test]
fn missing_pipe_is_rejected() {
    let err = parse_err("/mail WHERE id = 1");
    // No `|>` before WHERE: trailing input is unexpected.
    assert!(!err.expected.is_empty());
}

#[test]
fn dangling_where_is_eof() {
    let err = parse_err("/mail |> WHERE");
    assert_eq!(err.code, ParseErrorCode::UnexpectedEof);
}

/// blueprint decision O (ticket t70): a lone `=` always binds; it is never equality. A
/// stale SQL-style `WHERE a = 1` must therefore fail, with a message that steers the
/// author (human or AI) to `==` for equivalence (blueprint §6 actionable-error contract).
#[test]
fn equals_as_comparison_is_rejected_steering_to_eqeq() {
    let err = parse_err("/t |> WHERE a = 1");
    assert_eq!(err.code, ParseErrorCode::UnexpectedToken);
    assert!(
        err.message.contains("=="),
        "message should steer to `==` for equivalence, got: {}",
        err.message
    );
    // The diagnostic points at the offending `=` (and never echoes any literal).
    let src = "/t |> WHERE a = 1";
    assert_eq!(&src[err.span.range()], "=");
}

/// The migrated comparison form `WHERE x == 5` parses as an equality predicate, while
/// the binding `=` keeps working in `EXTEND`/`SET` — the two roles never collide
/// (decision O, t70).
#[test]
fn eqeq_compares_while_eq_still_binds() {
    let stmt = parse_ok("/t |> WHERE x == 5");
    let Statement::Query(p) = stmt else { panic!() };
    let PipeOp::Where(Expr::Binary { op: Op::Eq, .. }) = &p.ops[0] else {
        panic!(
            "`WHERE x == 5` should parse as an Eq comparison, got {:?}",
            p.ops[0]
        )
    };
    // `=` is still the assignment/binding token in EXTEND and SET.
    let stmt = parse_ok("/t |> EXTEND y = x |> SET z = 1");
    let Statement::Query(p) = stmt else { panic!() };
    assert!(matches!(p.ops[0], PipeOp::Extend(_)));
    assert!(matches!(p.ops[1], PipeOp::Set(_)));
}

#[test]
fn empty_input_is_eof() {
    let err = parse_err("");
    assert_eq!(err.code, ParseErrorCode::UnexpectedEof);
    assert!(!err.expected.is_empty());
}

// ---- span fidelity & structured-error contract ----------------------------

#[test]
fn error_carries_span_and_nonempty_expected() {
    let err = parse_err("/mail |> BANANA");
    assert!(
        !err.expected.is_empty(),
        "expected-set must be non-empty (blueprint §6)"
    );
    // The span round-trips into the source region of the offending token.
    let src = "/mail |> BANANA";
    let slice = &src[err.span.range()];
    assert!(!slice.is_empty());
}

#[test]
fn error_display_does_not_echo_string_literal_value() {
    // blueprint §8: a diagnostic must not echo a literal's contents (secret hygiene).
    let err = parse_err("/mail |> WHERE secret = 'p@ssw0rd' BANANA");
    let shown = format!("{err}");
    assert!(
        !shown.contains("p@ssw0rd"),
        "error Display leaked a literal value: {shown}"
    );
}

// ---- no-vendor-leak audit (blueprint §11, G6) ------------------------------------

#[test]
fn no_vendor_type_in_public_api() {
    let err = parse_statement("").expect_err("empty input is an error");
    // ParseError is fully owned: clonable, comparable, displayable with no
    // parser-library type in scope.
    let cloned = err.clone();
    assert_eq!(err, cloned);
    let shown = format!("{err}");
    assert!(shown.contains("UNEXPECTED_EOF"));
}

#[test]
fn sees_frozen_keyword_set() {
    // `FROM` was removed from the closed core in t73 (decision R): the source position needs no
    // keyword. `WHERE` (and the rest of the frozen set) is unaffected.
    // Keywords are canonically lowercase (t74, decision S); `from` is gone in any case.
    assert!(!KEYWORDS.contains(&"from"));
    assert!(!KEYWORDS.contains(&"FROM"));
    assert!(KEYWORDS.contains(&"where"));
    assert_eq!(Keyword::Where.text(), "where");
}

// ----------------------------------------------------------------------------------------------
// ADR 0009 — the relational definition layer: CREATE TABLE / REMOVE TABLE (contextual idents).
// ----------------------------------------------------------------------------------------------

#[test]
fn create_table_desugars_to_a_catalog_insert() {
    let stmt = parse_ok(
        "CREATE TABLE /sql/shop/customers (id int PRIMARY KEY, email text UNIQUE, \
         joined timestamp NOT NULL)",
    );
    let Statement::Effect(e) = stmt else {
        panic!("expected the desugared Effect, got {stmt:?}")
    };
    assert_eq!(e.verb, EffectVerb::Insert);
    // The target is the table's CATALOG (the parent path), not the table itself.
    let segs: Vec<&str> = e.target.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segs, vec!["sql", "shop"]);

    let EffectBody::Values(v) = &e.body else {
        panic!("expected a Values body")
    };
    assert_eq!(
        v.columns.as_deref(),
        Some(&["name".to_string(), "columns".to_string()][..])
    );
    let row = &v.rows[0];
    assert!(matches!(&row[0], Expr::Lit(Literal::Str(s)) if s == "customers"));
    let Expr::Array(cols) = &row[1] else {
        panic!("expected the columns array")
    };
    assert_eq!(cols.len(), 3);

    // id int PRIMARY KEY → { name:'id', type:'int', nullable:true, primary_key:true, unique:false }
    let Expr::Struct(id) = &cols[0] else {
        panic!("expected a column struct")
    };
    let get = |fields: &Vec<(String, Expr)>, key: &str| -> Expr {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("missing field {key}"))
    };
    assert!(matches!(get(id, "name"), Expr::Lit(Literal::Str(s)) if s == "id"));
    assert!(matches!(get(id, "type"), Expr::Lit(Literal::Str(s)) if s == "int"));
    assert!(matches!(
        get(id, "primary_key"),
        Expr::Lit(Literal::Bool(true))
    ));

    // email text UNIQUE → unique:true, nullable:true
    let Expr::Struct(email) = &cols[1] else {
        panic!("expected a column struct")
    };
    assert!(matches!(
        get(email, "unique"),
        Expr::Lit(Literal::Bool(true))
    ));
    assert!(matches!(
        get(email, "nullable"),
        Expr::Lit(Literal::Bool(true))
    ));

    // joined timestamp NOT NULL → nullable:false
    let Expr::Struct(joined) = &cols[2] else {
        panic!("expected a column struct")
    };
    assert!(matches!(
        get(joined, "nullable"),
        Expr::Lit(Literal::Bool(false))
    ));
    assert!(matches!(get(joined, "type"), Expr::Lit(Literal::Str(s)) if s == "timestamp"));
}

#[test]
fn create_table_of_type_desugars_to_a_catalog_insert_with_type_contract() {
    let stmt = parse_ok("CREATE TABLE /sql/shop/customers OF customer");
    let Statement::Effect(e) = stmt else {
        panic!("expected the desugared Effect, got {stmt:?}")
    };
    assert_eq!(e.verb, EffectVerb::Insert);
    let segs: Vec<&str> = e.target.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segs, vec!["sql", "shop"]);

    let EffectBody::Values(v) = &e.body else {
        panic!("expected a Values body")
    };
    assert_eq!(
        v.columns.as_deref(),
        Some(&["name".to_string(), "of_type".to_string()][..])
    );
    let row = &v.rows[0];
    assert!(matches!(&row[0], Expr::Lit(Literal::Str(s)) if s == "customers"));
    assert!(matches!(&row[1], Expr::Lit(Literal::Str(s)) if s == "/type/customer"));
}

#[test]
fn create_type_column_declared_type_name_is_canonicalized() {
    let e = effect_of(parse_ok(
        "CREATE TYPE customer (id int PRIMARY KEY, email email UNIQUE)",
    ));
    let body = values_cell(&e, "body").expect("columns json");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("object body");
    let columns = parsed
        .get("columns")
        .and_then(|c| c.as_array())
        .expect("columns");
    let email = columns
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("email"))
        .expect("email column");
    assert_eq!(
        email.get("type").and_then(|t| t.as_str()),
        Some("/type/email")
    );
}

#[test]
fn remove_table_desugars_to_a_catalog_remove_with_name_filter() {
    let stmt = parse_ok("REMOVE TABLE /sql/shop/customers");
    let Statement::Effect(e) = stmt else {
        panic!("expected the desugared Effect")
    };
    assert_eq!(e.verb, EffectVerb::Remove);
    let segs: Vec<&str> = e.target.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segs, vec!["sql", "shop"]);
    let EffectBody::SetWhere { set, filter } = &e.body else {
        panic!("expected a SetWhere body")
    };
    assert!(set.is_empty());
    let Some(Expr::Binary { op, lhs, rhs }) = filter.as_ref() else {
        panic!("expected the name == '<table>' filter")
    };
    assert_eq!(*op, Op::Eq);
    assert!(matches!(lhs.as_ref(), Expr::Col(c) if c == "name"));
    assert!(matches!(rhs.as_ref(), Expr::Lit(Literal::Str(s)) if s == "customers"));
}

#[test]
fn create_transform_desugars_to_a_transform_insert() {
    let stmt = parse_ok(
        "CREATE TRANSFORM classify INPUT (subject text, body text) \
         OUTPUT (category text, score float) PROVIDER claude MODEL 'claude-sonnet-5' \
         EFFORT medium SECRET 'vault:models/key'",
    );
    let Statement::Effect(e) = stmt else {
        panic!("expected the desugared Effect, got {stmt:?}")
    };
    assert_eq!(e.verb, EffectVerb::Insert);
    let segs: Vec<&str> = e.target.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segs, vec!["transform"]);
    let EffectBody::Values(v) = &e.body else {
        panic!("expected a Values body")
    };
    assert_eq!(
        v.columns.as_deref(),
        Some(
            &[
                "name".to_string(),
                "input".to_string(),
                "output".to_string(),
                "provider".to_string(),
                "model".to_string(),
                "effort".to_string(),
                "secret_ref".to_string(),
            ][..]
        )
    );
    let row = &v.rows[0];
    let lit = |e: &Expr| match e {
        Expr::Lit(Literal::Str(s)) => s.clone(),
        other => panic!("expected a string literal, got {other:?}"),
    };
    assert_eq!(lit(&row[0]), "classify");
    // INPUT/OUTPUT are column-descriptor JSON with canonical type strings.
    assert!(lit(&row[1]).contains("\"subject\"") && lit(&row[1]).contains("\"text\""));
    assert!(lit(&row[2]).contains("\"category\"") && lit(&row[2]).contains("\"score\""));
    assert_eq!(lit(&row[3]), "claude");
    assert_eq!(lit(&row[4]), "claude-sonnet-5");
    assert_eq!(lit(&row[5]), "medium");
    assert_eq!(lit(&row[6]), "vault:models/key");
}

#[test]
fn create_transform_input_is_optional_null_effort_and_secret() {
    // EFFORT and SECRET are optional: absent → NULL literals.
    let stmt =
        parse_ok("CREATE TRANSFORM t INPUT (body text) OUTPUT (label text) PROVIDER p MODEL m");
    let Statement::Effect(e) = stmt else {
        panic!("expected Effect")
    };
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values")
    };
    assert!(matches!(&v.rows[0][5], Expr::Lit(Literal::Null)));
    assert!(matches!(&v.rows[0][6], Expr::Lit(Literal::Null)));
}

#[test]
fn create_transform_relation_wise_input_carries_array_struct_canonical_type() {
    let stmt = parse_ok(
        "CREATE TRANSFORM r INPUT (rows array<struct<sku text, qty int>>) \
         OUTPUT (n int) PROVIDER p MODEL m",
    );
    let Statement::Effect(e) = stmt else {
        panic!("expected Effect")
    };
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values")
    };
    let Expr::Lit(Literal::Str(input_json)) = &v.rows[0][1] else {
        panic!("expected input JSON literal")
    };
    // The canonical type string the backend's ColumnType::parse reads back.
    assert!(
        input_json.contains("array<struct<sku:text,qty:int>>"),
        "input JSON should carry the canonical nested type, got {input_json}"
    );
}

#[test]
fn create_transform_empty_struct_parses_to_the_canonical_form() {
    // `struct<>` — the adjacent `<>` lexes as a single Ne token, and it is the exact canonical
    // string `ColumnType::parse` accepts (the decoder side), so the grammar (the encoder side)
    // must accept it too: encoder and decoder agree on the empty record.
    let stmt =
        parse_ok("CREATE TRANSFORM t INPUT (x struct<>) OUTPUT (label text) PROVIDER p MODEL m");
    let Statement::Effect(e) = stmt else {
        panic!("expected Effect")
    };
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values")
    };
    let Expr::Lit(Literal::Str(input_json)) = &v.rows[0][1] else {
        panic!("expected input JSON literal")
    };
    assert!(
        input_json.contains("struct<>"),
        "input JSON should carry the canonical empty struct, got {input_json}"
    );
    // Nested: an empty struct as a struct field breaks the same way without the Ne fix.
    let stmt = parse_ok(
        "CREATE TRANSFORM n INPUT (x struct<a struct<>>) OUTPUT (label text) PROVIDER p MODEL m",
    );
    let Statement::Effect(e) = stmt else {
        panic!("expected Effect")
    };
    let EffectBody::Values(v) = &e.body else {
        panic!("expected Values")
    };
    let Expr::Lit(Literal::Str(input_json)) = &v.rows[0][1] else {
        panic!("expected input JSON literal")
    };
    assert!(
        input_json.contains("struct<a:struct<>>"),
        "input JSON should carry the canonical nested empty struct, got {input_json}"
    );
}

#[test]
fn create_transform_rejects_an_inline_secret() {
    // A SECRET must be an env:/vault: REFERENCE — an inline value is a parse error.
    assert!(parse_statement(
        "CREATE TRANSFORM t INPUT (body text) OUTPUT (label text) PROVIDER p MODEL m \
         SECRET 'sk-inline-secret'"
    )
    .is_err());
    // A proper vault reference parses.
    assert!(parse_statement(
        "CREATE TRANSFORM t INPUT (body text) OUTPUT (label text) PROVIDER p MODEL m \
         SECRET 'env:MODEL_KEY'"
    )
    .is_ok());
}

#[test]
fn create_transform_requires_input_output_provider_model() {
    // Missing OUTPUT/PROVIDER/MODEL is rejected (a definition must declare its shape + model).
    assert!(parse_statement("CREATE TRANSFORM t INPUT (body text)").is_err());
    assert!(
        parse_statement("CREATE TRANSFORM t INPUT (body text) OUTPUT (label text) PROVIDER p")
            .is_err()
    );
}

#[test]
fn remove_transform_desugars_to_a_transform_remove_with_name_filter() {
    let stmt = parse_ok("REMOVE TRANSFORM classify");
    let Statement::Effect(e) = stmt else {
        panic!("expected the desugared Effect")
    };
    assert_eq!(e.verb, EffectVerb::Remove);
    let segs: Vec<&str> = e.target.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segs, vec!["transform"]);
    let EffectBody::SetWhere { set, filter } = &e.body else {
        panic!("expected a SetWhere body")
    };
    assert!(set.is_empty());
    let Some(Expr::Binary { op, lhs, rhs }) = filter.as_ref() else {
        panic!("expected the name == '<name>' filter")
    };
    assert_eq!(*op, Op::Eq);
    assert!(matches!(lhs.as_ref(), Expr::Col(c) if c == "name"));
    assert!(matches!(rhs.as_ref(), Expr::Lit(Literal::Str(s)) if s == "classify"));
}

#[test]
fn create_table_requires_a_catalog_qualified_path_and_columns() {
    // Too-short path (no connection segment) is rejected.
    assert!(parse_statement("CREATE TABLE /customers (id int)").is_err());
    // An empty column list is rejected.
    assert!(parse_statement("CREATE TABLE /sql/shop/customers ()").is_err());
}

#[test]
fn create_view_still_parses_as_server_ddl_not_a_table() {
    // The CREATE family dispatch must stay intact: VIEW is the frozen server-DDL noun.
    let stmt = parse_ok("CREATE VIEW top_orders AS /sql/shop/orders |> limit 5");
    assert!(matches!(stmt, Statement::Ddl(_)), "got {stmt:?}");
}
