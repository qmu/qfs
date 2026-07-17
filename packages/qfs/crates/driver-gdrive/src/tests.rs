//! Drive driver tests (blueprint §6 acceptance) — **no live Drive, no network, no credentials**.
//! Every test drives the introspective `Driver` surface and the apply leg against an in-memory
//! [`MockDriveClient`] (scripted Drive fixtures + recorded calls), so we assert request shape +
//! response decoding + plan shape + token-safety without touching a socket.

use std::sync::Arc;

use qfs_codec::JsonCodec;
use qfs_driver::{check_capability, resolve_proc, Archetype, Driver, Path, Verb, VersionSupport};
use qfs_plan::{
    preview, DriverId, EffectKind, EffectNode, NodeId, PlanBuilder, ProcId, Target, VfsPath,
};
use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};
use qfs_types::{Column, Row, RowBatch, Schema, Value};

use super::*;
use crate::client::FilePage;

// ---- fixtures ----------------------------------------------------------------------------

/// A driver over a mock seeded with one folder, one binary file, one Google doc, and a Shared
/// Drive.
fn driver_with_mock() -> (GDriveDriver, Arc<MockDriveClient>) {
    let mock = Arc::new(
        MockDriveClient::new()
            .with_file(FileMeta {
                id: "folder1".to_string(),
                name: "reports".to_string(),
                mime_type: FOLDER_MIME.to_string(),
                parents: vec!["root".to_string()],
                size: 0,
                modified_time: 0,
                md5: String::new(),
                rev: String::new(),
                drive_id: String::new(),
                trashed: false,
            })
            .with_file(FileMeta {
                id: "f1".to_string(),
                name: "data.json".to_string(),
                mime_type: "application/json".to_string(),
                parents: vec!["folder1".to_string()],
                size: 17,
                modified_time: 1_700_000_000_000,
                md5: "abc".to_string(),
                rev: "rev9".to_string(),
                drive_id: String::new(),
                trashed: false,
            })
            .with_file(FileMeta {
                id: "doc1".to_string(),
                name: "notes".to_string(),
                mime_type: "application/vnd.google-apps.document".to_string(),
                parents: vec!["folder1".to_string()],
                size: 0,
                modified_time: 1_700_000_000_000,
                md5: String::new(),
                rev: "rev1".to_string(),
                drive_id: String::new(),
                trashed: false,
            })
            .with_drive(SharedDrive::for_test("d1", "team"))
            .with_download("f1", br#"[{"a":1},{"a":2}]"#.to_vec())
            .with_download("doc1", b"exported-bytes".to_vec()),
    );
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    (driver, mock)
}

fn target(path: &str) -> Target {
    Target::new(DriverId::new("drive"), VfsPath::new(path))
}

/// Build a single-row args batch (the columns the effect decoder reads).
fn args(cols: &[(&str, Value)]) -> RowBatch {
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
fn mount_and_id_are_drive() {
    let (d, _) = driver_with_mock();
    assert_eq!(d.mount(), "/drive");
    assert_eq!(d.id(), DriverId::new("drive"));
}

#[test]
fn describe_emits_blob_archetype_and_file_schema() {
    let (d, _) = driver_with_mock();
    let desc = d.describe(&Path::new("/drive/my/reports")).unwrap();
    assert_eq!(desc.archetype, Archetype::BlobNamespace);
    for col in [
        "id",
        "name",
        "mime_type",
        "parents",
        "size",
        "modified_time",
        "md5",
        "is_google_doc",
        "rev",
        "drive_id",
        "trashed",
        // The unified content column: describe advertises it (nullable) so a single-file
        // `/drive/<file> |> select content |> transform` type-checks at plan time.
        "content",
    ] {
        assert!(desc.schema.column(col).is_some(), "missing column {col}");
    }
    assert_eq!(
        desc.schema.column("modified_time").unwrap().ty,
        qfs_types::ColumnType::Timestamp
    );
    assert_eq!(
        desc.schema.column("content").unwrap().ty,
        qfs_types::ColumnType::Bytes
    );
    assert!(
        desc.schema.column("content").unwrap().nullable,
        "content is nullable (a folder listing carries a null content)"
    );
    // Drive versions files (blueprint §4 @rev).
    assert_eq!(
        d.version_support(&Path::new("/drive/my/reports/data.json")),
        VersionSupport::Versioned
    );
}

// ---- capability golden (path-keyed gate) -------------------------------------------------

#[test]
fn capabilities_are_path_keyed() {
    let (d, _) = driver_with_mock();

    // The My Drive corpus root is a WRITABLE folder (gdrive-ftp puts/mkdirs at the top level):
    // ls/select plus insert/upsert — but the root itself cannot be renamed or trashed.
    let my_root = Path::new("/drive/my");
    assert!(check_capability(&d, &my_root, Verb::Ls).is_ok());
    assert!(check_capability(&d, &my_root, Verb::Select).is_ok());
    assert!(check_capability(&d, &my_root, Verb::Insert).is_ok());
    assert!(check_capability(&d, &my_root, Verb::Upsert).is_ok());
    // REMOVE at the root is trash-by-name only (the bare form fails closed in the decode).
    assert!(check_capability(&d, &my_root, Verb::Remove).is_ok());
    assert!(check_capability(&d, &my_root, Verb::Update).is_err());

    // The virtual root and the Shared-Drives listing root stay read-only.
    assert!(check_capability(&d, &Path::new("/drive"), Verb::Insert).is_err());
    assert!(check_capability(&d, &Path::new("/drive/shared"), Verb::Insert).is_err());

    // A folder path: ls/select/insert/upsert/remove/cp/mv.
    let folder = Path::new("/drive/my/reports");
    assert!(check_capability(&d, &folder, Verb::Insert).is_ok());
    assert!(check_capability(&d, &folder, Verb::Upsert).is_ok());
    assert!(check_capability(&d, &folder, Verb::Mv).is_ok());

    // A file by id: select/upsert/update/remove/cp/mv, NOT a relational insert of columns.
    let file = Path::new("id:f1");
    assert!(check_capability(&d, &file, Verb::Select).is_ok());
    assert!(check_capability(&d, &file, Verb::Remove).is_ok());
    let err = check_capability(&d, &file, Verb::Insert).unwrap_err();
    assert_eq!(err.code(), "unsupported_verb");
}

#[test]
fn insert_of_columns_into_a_file_is_rejected_structurally() {
    let (d, _) = driver_with_mock();
    let err = check_capability(&d, &Path::new("id:f1"), Verb::Insert).unwrap_err();
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

// ---- procedures (drive.copy) -------------------------------------------------------------

#[test]
fn drive_copy_is_declared_with_drive_scope() {
    let (d, _) = driver_with_mock();
    let copy = resolve_proc(&d, "copy").unwrap();
    assert!(!copy.irreversible, "a copy creates, never destroys");
    assert_eq!(copy.requires_scopes, vec![DRIVE_SCOPE.to_string()]);
    assert_eq!(
        resolve_proc(&d, "nuke").unwrap_err().code(),
        "unknown_procedure"
    );
}

// ---- path parsing: my / shared / id / @rev ----------------------------------------------

#[test]
fn paths_parse_to_corpora_drives_ids_and_revisions() {
    assert_eq!(DrivePath::parse_str("/drive").unwrap(), DrivePath::Root);
    assert_eq!(
        DrivePath::parse_str("/drive/my").unwrap(),
        DrivePath::MyRoot
    );
    assert_eq!(
        DrivePath::parse_str("/drive/shared").unwrap(),
        DrivePath::SharedRoot
    );
    match DrivePath::parse_str("/drive/my/a/b.txt").unwrap() {
        DrivePath::My { segments, revision } => {
            assert_eq!(segments, vec!["a".to_string(), "b.txt".to_string()]);
            assert!(revision.is_none());
        }
        other => panic!("expected My, got {other:?}"),
    }
    // A @rev suffix on the last segment pins a revision.
    match DrivePath::parse_str("/drive/my/a/b.txt@rev7").unwrap() {
        DrivePath::My { segments, revision } => {
            assert_eq!(segments.last().unwrap(), "b.txt");
            assert_eq!(revision.as_deref(), Some("rev7"));
        }
        other => panic!("expected My with revision, got {other:?}"),
    }
    // Shared Drive path names the drive.
    match DrivePath::parse_str("/drive/shared/team/x").unwrap() {
        DrivePath::Shared {
            drive, segments, ..
        } => {
            assert_eq!(drive, "team");
            assert_eq!(segments, vec!["x".to_string()]);
        }
        other => panic!("expected Shared, got {other:?}"),
    }
    // id:<fileId>@rev.
    match DrivePath::parse_str("id:f1@rev9").unwrap() {
        DrivePath::ById { id, revision } => {
            assert_eq!(id, "f1");
            assert_eq!(revision.as_deref(), Some("rev9"));
        }
        other => panic!("expected ById, got {other:?}"),
    }
    // The MOUNTED id-selector `/drive/id:<fileId>` (the cookbook's documented escape hatch) — the
    // round-5 defect returned `invalid_path` for this spelling; it must parse to the same ById.
    match DrivePath::parse_str("/drive/id:1xVtAbC").unwrap() {
        DrivePath::ById { id, revision } => {
            assert_eq!(id, "1xVtAbC");
            assert!(revision.is_none());
        }
        other => panic!("expected ById from the mounted spelling, got {other:?}"),
    }
    match DrivePath::parse_str("/drive/id:1xVtAbC@rev3").unwrap() {
        DrivePath::ById { id, revision } => {
            assert_eq!(id, "1xVtAbC");
            assert_eq!(revision.as_deref(), Some("rev3"));
        }
        other => panic!("expected mounted ById with revision, got {other:?}"),
    }
    // An invalid corpus is rejected.
    assert_eq!(
        DrivePath::parse_str("/drive/bogus").unwrap_err().code(),
        "invalid_path"
    );
}

#[test]
fn a_space_named_file_is_readable_by_the_mounted_id() {
    // Round-5: a model-chosen name with spaces cannot be a path segment (`… comparison.pdf` →
    // parse UNEXPECTED_TOKEN), so `/drive/id:<id>` is the only way to reach it. Prove the content
    // reads through that mounted-id address (which previously returned `invalid_path`).
    let mock = Arc::new(
        MockDriveClient::new()
            .with_file(FileMeta::for_test(
                "space1",
                "Fundamental LLM model comparison.pdf",
                "application/pdf",
                vec!["folder1".to_string()],
            ))
            .with_download("space1", b"%PDF-1.7 body".to_vec()),
    );
    let batch = read_rows(mock.as_ref() as &dyn GDriveClient, "/drive/id:space1", None).unwrap();
    assert_eq!(
        batch.rows.len(),
        1,
        "the id-addressed file reads one content row"
    );
    let content = batch.rows[0].values.last().expect("content column");
    assert_eq!(
        content,
        &Value::Bytes(b"%PDF-1.7 body".to_vec()),
        "the space-named file's bytes read via /drive/id:<id>"
    );
}

// ---- pushdown: WHERE → Drive q, TRUTHFUL residual (the t20 lesson) ------------------------

#[test]
fn where_lowers_to_drive_q_with_lossy_residual_kept_local() {
    use qfs_types::{CmpOp, ColRef, Literal, Predicate};
    // name = 'report.txt' AND name ~ 'rep' AND mime_type = 'text/plain'.
    // `name =` and `mimeType =` are EXACT (drop from residual); `name ~` lowers to the loose
    // `name contains` pre-filter and MUST be kept as residual (over-fetch then filter).
    let name_eq = Predicate::Cmp(
        ColRef::col("name"),
        CmpOp::Eq,
        Literal::Text("report.txt".to_string()),
    );
    let name_match = Predicate::Cmp(
        ColRef::col("name"),
        CmpOp::Match,
        Literal::Text("rep".to_string()),
    );
    let mime_eq = Predicate::Cmp(
        ColRef::col("mime_type"),
        CmpOp::Eq,
        Literal::Text("text/plain".to_string()),
    );
    let pred = Predicate::And(
        Box::new(name_eq.clone()),
        Box::new(Predicate::And(
            Box::new(name_match.clone()),
            Box::new(mime_eq.clone()),
        )),
    );
    let res = query::build_query(Some("folder1"), Some(&pred));
    assert_eq!(
        res.query,
        "'folder1' in parents and name = 'report.txt' and name contains 'rep' and mimeType = 'text/plain'"
    );
    // Only the lossy `name ~ 'rep'` is kept as residual; the two exact terms drop out.
    assert_eq!(
        res.residual,
        Some(name_match),
        "lossy name-contains pre-filter is kept as residual; exact name=/mimeType= drop out"
    );

    let (d, _) = driver_with_mock();
    assert!(d.pushdown().supports_where());
    assert!(d.pushdown().supports_limit());
    assert!(!d.pushdown().supports_order());
}

#[test]
fn exact_predicates_push_fully_with_no_residual() {
    use qfs_types::{CmpOp, ColRef, Literal, Predicate};
    // mime_type = 'folder' AND trashed = false: both EXACT, so the whole predicate pushes and
    // nothing is left to re-check locally — residual is None.
    let pred = Predicate::And(
        Box::new(Predicate::Cmp(
            ColRef::col("mime_type"),
            CmpOp::Eq,
            Literal::Text(FOLDER_MIME.to_string()),
        )),
        Box::new(Predicate::Cmp(
            ColRef::col("trashed"),
            CmpOp::Eq,
            Literal::Bool(false),
        )),
    );
    let res = query::build_query(None, Some(&pred));
    assert_eq!(
        res.query,
        format!("mimeType = '{FOLDER_MIME}' and trashed = false")
    );
    assert!(
        res.residual.is_none(),
        "exact mimeType/trashed mappings leave no residual to re-check"
    );
}

#[test]
fn lossy_predicate_returns_residual_so_engine_refilters() {
    use qfs_types::{CmpOp, ColRef, Literal, Pattern, Predicate};

    // name LIKE 'weekly' pushes the loose `name contains 'weekly'` but MUST keep the exact LIKE
    // as residual: Drive `contains` also matches "weekly-2024.txt" / a token in a longer name.
    let like = Predicate::Like(ColRef::col("name"), Pattern("weekly".to_string()));
    let res = query::build_query(None, Some(&like));
    assert_eq!(res.query, "name contains 'weekly'");
    assert_eq!(
        res.residual,
        Some(like),
        "LIKE has no Drive operator — `contains` is looser, so kept residual"
    );

    // A modified_time bound is second-granular RFC-3339 — looser than the exact ms comparison,
    // so it is kept as residual.
    let date_gt = Predicate::Cmp(
        ColRef::col("modified_time"),
        CmpOp::Gt,
        Literal::Int(1_700_000_500_500),
    );
    let res = query::build_query(None, Some(&date_gt));
    assert_eq!(res.query, "modifiedTime > '2023-11-14T22:21:40Z'");
    assert_eq!(
        res.residual,
        Some(date_gt),
        "the modifiedTime bound is second-granular/truncated — kept residual for the exact ms compare"
    );

    // An OR stays wholly residual (Drive `and`-joining cannot express it).
    let or_pred = Predicate::Or(
        Box::new(Predicate::Cmp(
            ColRef::col("name"),
            CmpOp::Eq,
            Literal::Text("a".into()),
        )),
        Box::new(Predicate::Cmp(
            ColRef::col("name"),
            CmpOp::Eq,
            Literal::Text("b".into()),
        )),
    );
    let res = query::build_query(None, Some(&or_pred));
    assert_eq!(res.query, "", "nothing pushed for an OR");
    assert!(res.residual.is_some(), "OR is residual, filtered locally");
}

// ---- list/search reaches the client + decodes file rows ----------------------------------

#[test]
fn list_files_pushes_q_and_decodes_file_rows() {
    let (_d, mock) = driver_with_mock();
    let client: &dyn GDriveClient = &*mock;
    let _ = mock; // bind
                  // Seed a page and list it with a pushed q + Shared-Drive scope.
    let page = FilePage {
        files: vec![FileMeta::for_test(
            "f1",
            "data.json",
            "application/json",
            vec!["folder1".to_string()],
        )],
        next_page_token: None,
    };
    // re-seed through a fresh mock to control the page queue deterministically
    let mock2 = Arc::new(MockDriveClient::new().with_list_page(page));
    let client2: &dyn GDriveClient = &*mock2;
    let res = client2
        .list_files("'folder1' in parents", Some("d1"), Some(50))
        .unwrap();
    assert_eq!(res.files.len(), 1);
    let row = res.files[0].to_row();
    assert_eq!(row.values[0], Value::Text("f1".to_string()));
    assert_eq!(row.values[1], Value::Text("data.json".to_string()));
    // The mock recorded the exact q + Shared-Drive id + page size.
    assert!(mock2.recorded().contains(&RecordedCall::ListFiles {
        query: "'folder1' in parents".to_string(),
        drive_id: Some("d1".to_string()),
        page_size: Some(50),
    }));
    let _ = client; // bound to prove the seeded mock is distinct.
}

#[test]
fn list_drives_returns_shared_drives() {
    let (_d, mock) = driver_with_mock();
    let client: &dyn GDriveClient = &*mock;
    let drives = client.list_drives().unwrap();
    assert_eq!(drives, vec![SharedDrive::for_test("d1", "team")]);
    assert!(mock.recorded().contains(&RecordedCall::ListDrives));
}

// ---- read path: download+decode, and export of a google doc ------------------------------

#[test]
fn download_binary_then_decode_to_rows_via_codec() {
    let (_d, mock) = driver_with_mock();
    let client: &dyn GDriveClient = &*mock;
    let file = client.get_file("f1").unwrap();
    // A binary file plans a raw download.
    let plan = plan_read(&file, None, None).unwrap();
    assert_eq!(
        plan,
        ReadPlan::Download {
            id: "f1".to_string(),
            revision: None
        }
    );
    let bytes = client.download("f1", None).unwrap();
    // Decode the JSON body to rows via the codec (the bytes → rows boundary).
    let batch = decode_body(&JsonCodec, &bytes).unwrap();
    assert_eq!(batch.rows.len(), 2);
    assert!(mock.recorded().contains(&RecordedCall::Download {
        id: "f1".to_string(),
        revision: None,
    }));
}

#[test]
fn google_doc_plans_an_export_to_docx_by_default_and_honors_override() {
    let (_d, mock) = driver_with_mock();
    let client: &dyn GDriveClient = &*mock;
    let doc = client.get_file("doc1").unwrap();
    assert!(doc.is_google_doc(), "a google doc has no raw bytes");
    // Default export → docx.
    let plan = plan_read(&doc, None, None).unwrap();
    match plan {
        ReadPlan::Export { id, target } => {
            assert_eq!(id, "doc1");
            assert_eq!(target.suffix, "docx");
            assert!(target.mime.contains("wordprocessingml"));
        }
        other => panic!("expected Export, got {other:?}"),
    }
    // An explicit override → pdf.
    let plan = plan_read(&doc, None, Some("pdf")).unwrap();
    match plan {
        ReadPlan::Export { target, .. } => {
            assert_eq!(target.mime, "application/pdf");
            assert_eq!(target.suffix, "pdf");
        }
        other => panic!("expected Export, got {other:?}"),
    }
    // The export call reaches the client.
    let _ = client.export("doc1", "application/pdf").unwrap();
    assert!(mock.recorded().contains(&RecordedCall::Export {
        id: "doc1".to_string(),
        export_mime: "application/pdf".to_string(),
    }));
}

// ---- effect decode: upload / upsert / move / trash / hard-delete / copy -------------------

#[test]
fn insert_into_folder_decodes_to_upload() {
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/drive/my/reports/x.txt"),
    )
    .with_args(args(&[
        (PARENT_ID_COL, Value::Text("folder1".into())),
        (MIME_COL, Value::Text("text/plain".into())),
        (BYTES_COL, Value::Bytes(b"hi".to_vec())),
    ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Upload {
            parent,
            name,
            mime,
            bytes,
        } => {
            assert_eq!(parent, "folder1");
            assert_eq!(name, "x.txt", "name falls back to the path leaf segment");
            assert_eq!(mime, "text/plain");
            assert_eq!(bytes, b"hi");
        }
        other => panic!("expected Upload, got {other:?}"),
    }
}

#[test]
fn insert_of_a_folder_decodes_to_a_byteless_upload() {
    // gmail-ftp `mkdir`: an INSERT carrying the folder mime and no bytes decodes to an Upload the
    // real client sends as a **metadata-only** files.create (no media part).
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/drive/my/reports"))
        .with_args(args(&[
            (PARENT_ID_COL, Value::Text("folder1".into())),
            (NAME_COL, Value::Text("Q3".into())),
            (MIME_COL, Value::Text(FOLDER_MIME.to_string())),
        ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Upload {
            name, mime, bytes, ..
        } => {
            assert_eq!(name, "Q3");
            assert_eq!(mime, FOLDER_MIME);
            assert!(bytes.is_empty(), "a folder carries no bytes");
        }
        other => panic!("expected Upload, got {other:?}"),
    }
}

#[test]
fn upload_reads_the_well_known_content_blob_column() {
    // A cross-driver `cp /local/x.pdf /drive/y.pdf` materializes the source file's bytes into the
    // engine's well-known `content` column (ticket 20260707181404) — NOT the drive-native `bytes`
    // column. The decoder must pick those up so the copy carries the real file, not zero bytes.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/drive/my/reports/x.pdf"),
    )
    .with_args(args(&[
        (PARENT_ID_COL, Value::Text("folder1".into())),
        (NAME_COL, Value::Text("x.pdf".into())),
        ("content", Value::Bytes(b"%PDF-1.7".to_vec())),
    ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Upload { bytes, .. } => assert_eq!(bytes, b"%PDF-1.7"),
        other => panic!("expected Upload carrying the content bytes, got {other:?}"),
    }
}

#[test]
fn empty_source_file_is_a_valid_zero_byte_upload() {
    // An explicit empty `content` value is a genuinely empty source file — a valid zero-byte
    // upload, NOT the silent-truncation bug (the guard fails closed only on a MISSING payload).
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/drive/my/reports/empty.txt"),
    )
    .with_args(args(&[
        (PARENT_ID_COL, Value::Text("folder1".into())),
        (NAME_COL, Value::Text("empty.txt".into())),
        ("content", Value::Bytes(Vec::new())),
    ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Upload { bytes, .. } => assert!(bytes.is_empty(), "empty file stays empty"),
        other => panic!("expected Upload, got {other:?}"),
    }
}

#[test]
fn byteless_file_upload_fails_closed() {
    // The materialization gap the ticket reproduces: an INSERT of a FILE that carries NO
    // `content`/`bytes` payload must REFUSE rather than create a zero-byte Drive object. (Only a
    // metadata-only folder create may be byteless — asserted by the test above.)
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/drive/my/reports/x.pdf"),
    )
    .with_args(args(&[
        (PARENT_ID_COL, Value::Text("folder1".into())),
        (NAME_COL, Value::Text("x.pdf".into())),
    ]));
    let err = DriveEffect::from_node(&node).unwrap_err();
    assert_eq!(
        err.code(),
        "malformed_effect",
        "byteless file upload is refused"
    );
}

#[test]
fn content_replace_without_bytes_fails_closed() {
    // An UPSERT keyed by a resolved file id is a content replace; with no payload it would
    // truncate the existing file to empty. Fail closed instead (ticket 20260707181404).
    let node = EffectNode::new(NodeId(0), EffectKind::Upsert, target("id:f1")).with_args(args(&[
        (FILE_ID_COL, Value::Text("f1".into())),
        (MIME_COL, Value::Text("application/pdf".into())),
    ]));
    let err = DriveEffect::from_node(&node).unwrap_err();
    assert_eq!(
        err.code(),
        "malformed_effect",
        "byteless content replace is refused"
    );
}

#[test]
fn upsert_with_file_id_is_retry_safe_content_replace() {
    let node = EffectNode::new(NodeId(0), EffectKind::Upsert, target("id:f1")).with_args(args(&[
        (FILE_ID_COL, Value::Text("f1".into())),
        (MIME_COL, Value::Text("application/json".into())),
        (BYTES_COL, Value::Bytes(b"[]".to_vec())),
    ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Update { id, .. } => assert_eq!(id, "f1"),
        other => panic!("expected Update, got {other:?}"),
    }
}

#[test]
fn update_decodes_to_rename_and_move() {
    let node = EffectNode::new(NodeId(0), EffectKind::Update, target("id:f1")).with_args(args(&[
        (FILE_ID_COL, Value::Text("f1".into())),
        (NAME_COL, Value::Text("renamed.json".into())),
        (ADD_PARENTS_COL, Value::Text("folder2".into())),
        (REMOVE_PARENTS_COL, Value::Text("folder1".into())),
    ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Move {
            id,
            new_name,
            add_parents,
            remove_parents,
        } => {
            assert_eq!(id, "f1");
            assert_eq!(new_name.as_deref(), Some("renamed.json"));
            assert_eq!(add_parents, vec!["folder2".to_string()]);
            assert_eq!(remove_parents, vec!["folder1".to_string()]);
        }
        other => panic!("expected Move, got {other:?}"),
    }
}

/// A resolver whose `existing()` returns a fixed node — lets a decode test drive the
/// name-path resolution branch (folder vs file) without a live client.
struct FixedExisting(Option<FileMeta>);

impl crate::effect::WriteResolver for FixedExisting {
    fn folder_id(
        &self,
        _path: &crate::path::DrivePath,
        raw: &str,
    ) -> Result<(String, Option<String>), DriveError> {
        Err(DriveError::MalformedEffect {
            verb: "TEST",
            path: raw.to_string(),
            reason: "folder_id unused by these decode tests".to_string(),
        })
    }
    fn existing(
        &self,
        _path: &crate::path::DrivePath,
        _raw: &str,
    ) -> Result<Option<FileMeta>, DriveError> {
        Ok(self.0.clone())
    }
    fn child_id(&self, _parent_id: &str, _name: &str) -> Result<Option<String>, DriveError> {
        Ok(None)
    }
}

#[test]
fn update_set_name_on_a_folder_name_path_refuses_the_wrong_node_write() {
    // Round-5 defect: `UPDATE /drive/my/<folder> SET name = 'x' WHERE name == '<file>'` collapsed
    // to a bare `SET name` on the FOLDER path — the WHERE key is dropped when it shares the SET
    // column — and silently renamed the CONTAINER. A folder rename reached by NAME path must now
    // refuse loudly rather than mutate the wrong node.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Update,
        target("/drive/my/qfs-extract-test"),
    )
    .with_args(args(&[(NAME_COL, Value::Text("extracted.txt".into()))]));
    let res = FixedExisting(Some(FileMeta::for_test(
        "folderX",
        "qfs-extract-test",
        FOLDER_MIME,
        vec![],
    )));
    let err = DriveEffect::from_node_with(&node, &res).unwrap_err();
    assert_eq!(
        err.code(),
        "malformed_effect",
        "renaming a folder reached by name path is refused loudly, not applied to the folder"
    );
}

#[test]
fn update_set_name_on_a_file_name_path_renames_the_file() {
    // The safe counterpart: the same statement shape against a FILE path resolves that file and
    // renames it — no folder is involved, nothing is refused.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Update,
        target("/drive/my/report.txt"),
    )
    .with_args(args(&[(NAME_COL, Value::Text("renamed.txt".into()))]));
    let res = FixedExisting(Some(FileMeta::for_test(
        "fileY",
        "report.txt",
        "text/plain",
        vec!["folder1".into()],
    )));
    match DriveEffect::from_node_with(&node, &res).unwrap() {
        DriveEffect::Move { id, new_name, .. } => {
            assert_eq!(id, "fileY");
            assert_eq!(new_name.as_deref(), Some("renamed.txt"));
        }
        other => panic!("expected Move on the resolved file, got {other:?}"),
    }
}

#[test]
fn update_renames_a_folder_addressed_by_id() {
    // The sanctioned way to rename a FOLDER itself: address it unambiguously by id. The
    // name-path guard does not apply, so the rename decodes normally.
    let node = EffectNode::new(NodeId(0), EffectKind::Update, target("id:folderX"))
        .with_args(args(&[(NAME_COL, Value::Text("renamed-folder".into()))]));
    let res = FixedExisting(Some(FileMeta::for_test(
        "folderX",
        "qfs-extract-test",
        FOLDER_MIME,
        vec![],
    )));
    match DriveEffect::from_node_with(&node, &res).unwrap() {
        DriveEffect::Move { id, new_name, .. } => {
            assert_eq!(id, "folderX");
            assert_eq!(new_name.as_deref(), Some("renamed-folder"));
        }
        other => panic!("expected Move on the id-addressed folder, got {other:?}"),
    }
}

/// A resolver whose `existing()` returns queued responses in call order — the folder-path UPDATE
/// with a `name` selector calls `existing()` twice (the folder, then the child), so the queue holds
/// `[folder, child]`. Lets a decode test drive the selector-child resolution without a live client.
struct SeqExisting(std::cell::RefCell<Vec<Result<Option<FileMeta>, DriveError>>>);

impl crate::effect::WriteResolver for SeqExisting {
    fn folder_id(
        &self,
        _path: &crate::path::DrivePath,
        raw: &str,
    ) -> Result<(String, Option<String>), DriveError> {
        Err(DriveError::MalformedEffect {
            verb: "TEST",
            path: raw.to_string(),
            reason: "folder_id unused by these decode tests".to_string(),
        })
    }
    fn existing(
        &self,
        _path: &crate::path::DrivePath,
        _raw: &str,
    ) -> Result<Option<FileMeta>, DriveError> {
        let mut q = self.0.borrow_mut();
        if q.is_empty() {
            return Ok(None);
        }
        q.remove(0)
    }
    fn child_id(&self, _parent_id: &str, _name: &str) -> Result<Option<String>, DriveError> {
        Ok(None)
    }
}

#[test]
fn update_folder_set_name_where_name_renames_the_matching_child() {
    // Ticket 20260713195008: `UPDATE /drive/my/<folder> SET name='new' WHERE name='old'` now renames
    // the CHILD, because the WHERE selector survives to the applier via `node.selector` distinct from
    // the SET `name` payload. The resolver returns the folder (path resolution) then the child (the
    // selector resolution), and the child's id is what the Move targets.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Update,
        target("/drive/my/qfs-extract-test"),
    )
    .with_args(args(&[(
        NAME_COL,
        Value::Text("extracted-renamed.txt".into()),
    )]))
    .with_selector(args(&[(NAME_COL, Value::Text("extracted.txt".into()))]));
    let res = SeqExisting(std::cell::RefCell::new(vec![
        Ok(Some(FileMeta::for_test(
            "folderX",
            "qfs-extract-test",
            FOLDER_MIME,
            vec![],
        ))),
        Ok(Some(FileMeta::for_test(
            "childZ",
            "extracted.txt",
            "text/plain",
            vec!["folderX".into()],
        ))),
    ]));
    match DriveEffect::from_node_with(&node, &res).unwrap() {
        DriveEffect::Move { id, new_name, .. } => {
            assert_eq!(
                id, "childZ",
                "the MATCHING CHILD is renamed, not the folder"
            );
            assert_eq!(new_name.as_deref(), Some("extracted-renamed.txt"));
        }
        other => panic!("expected Move on the resolved child, got {other:?}"),
    }
}

#[test]
fn update_folder_where_name_ambiguous_child_refuses() {
    // Codex caveat: child resolution must be ambiguity-safe. When the selector matches ≥2 children,
    // `existing()`/`resolve_node` returns `AmbiguousTarget` — the decode surfaces it, never renaming
    // an arbitrary duplicate.
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Update,
        target("/drive/my/qfs-extract-test"),
    )
    .with_args(args(&[(NAME_COL, Value::Text("new.txt".into()))]))
    .with_selector(args(&[(NAME_COL, Value::Text("dup.txt".into()))]));
    let res = SeqExisting(std::cell::RefCell::new(vec![
        Ok(Some(FileMeta::for_test(
            "folderX",
            "qfs-extract-test",
            FOLDER_MIME,
            vec![],
        ))),
        Err(DriveError::AmbiguousTarget {
            path: "/drive/my/qfs-extract-test/dup.txt".to_string(),
        }),
    ]));
    let err = DriveEffect::from_node_with(&node, &res).unwrap_err();
    assert_eq!(
        err.code(),
        "ambiguous_target",
        "a selector matching ≥2 children refuses rather than renaming an arbitrary one"
    );
}

#[test]
fn update_folder_with_non_name_selector_still_refuses() {
    // A selector with keys other than a single `name` cannot be resolved to one child, so the
    // name-path folder UPDATE stays the safe loud refusal (never mutates the container).
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Update,
        target("/drive/my/qfs-extract-test"),
    )
    .with_args(args(&[(NAME_COL, Value::Text("new.txt".into()))]))
    .with_selector(args(&[("mime_type", Value::Text("text/plain".into()))]));
    let res = FixedExisting(Some(FileMeta::for_test(
        "folderX",
        "qfs-extract-test",
        FOLDER_MIME,
        vec![],
    )));
    let err = DriveEffect::from_node_with(&node, &res).unwrap_err();
    assert_eq!(
        err.code(),
        "malformed_effect",
        "a non-`name` selector is not resolvable to one child; refuse loudly"
    );
}

/// A resolver reporting a fixed parent folder and a set of already-existing child names — drives
/// the UPSERT create-vs-replace probe (`child_id`).
struct FolderChildren {
    parent: String,
    existing: Vec<(String, String)>,
}
impl crate::effect::WriteResolver for FolderChildren {
    fn folder_id(
        &self,
        _path: &crate::path::DrivePath,
        _raw: &str,
    ) -> Result<(String, Option<String>), DriveError> {
        Ok((self.parent.clone(), None))
    }
    fn existing(
        &self,
        _path: &crate::path::DrivePath,
        _raw: &str,
    ) -> Result<Option<FileMeta>, DriveError> {
        Ok(None)
    }
    fn child_id(&self, _parent_id: &str, name: &str) -> Result<Option<String>, DriveError> {
        Ok(self
            .existing
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| id.clone()))
    }
}

#[test]
fn multi_row_folder_upsert_replaces_existing_and_creates_new_per_row() {
    // Ticket 20260712150000 (round-2 parity): UPSERT into a FOLDER path must decode one write per
    // row (INSERT parity) — replace where the name exists, create where it does not — instead of
    // refusing "bytes cannot replace a folder". This is what the INSERT-collision error's advice
    // ("use UPSERT to replace its content") relies on.
    let node = EffectNode::new(NodeId(0), EffectKind::Upsert, target("/drive/my/reports"))
        .with_args(multi_args(
            &[NAME_COL, BYTES_COL],
            &[
                &[
                    Value::Text("exists.txt".into()),
                    Value::Bytes(b"v2".to_vec()),
                ],
                &[
                    Value::Text("fresh.txt".into()),
                    Value::Bytes(b"new".to_vec()),
                ],
            ],
        ));
    let res = FolderChildren {
        parent: "folder1".to_string(),
        existing: vec![("exists.txt".to_string(), "existing-id".to_string())],
    };
    let effects = DriveEffect::from_node_rows_with(&node, &res).unwrap();
    assert_eq!(effects.len(), 2, "one effect per row (INSERT parity)");
    match &effects[0] {
        DriveEffect::Update { id, bytes, .. } => {
            assert_eq!(
                id, "existing-id",
                "the colliding name replaces content by id"
            );
            assert_eq!(bytes, b"v2");
        }
        other => panic!("expected Update (replace) for the existing name, got {other:?}"),
    }
    match &effects[1] {
        DriveEffect::Upload { parent, name, .. } => {
            assert_eq!(parent, "folder1", "the new file uploads into the folder");
            assert_eq!(name, "fresh.txt");
        }
        other => panic!("expected Upload (create) for the new name, got {other:?}"),
    }
}

#[test]
fn a_name_with_a_question_mark_and_spaces_is_addressable_as_a_single_file_path() {
    // Ticket 20260717120200 — the DRIVER half of the chain. The parser half
    // (`quoted_path_segment_addresses_a_single_file_remove`) proves that
    // `remove /drive/my/'Q3 budget?.xlsx'` renders to exactly the raw path used here; this proves
    // the driver then reads and trashes that one file. Together they close the loop the incident
    // could not walk: the safe single-file spelling is now writable AND it resolves.
    const NAME: &str = "Q3 budget (final)?.xlsx";
    let path = format!("/drive/my/{NAME}");
    let file = FileMeta::for_test("q3", NAME, "text/plain", vec!["root".to_string()]);

    // READ: the single-file path resolves and downloads the bytes.
    let client = MockDriveClient::new()
        .with_list_page(FilePage {
            files: vec![file.clone()],
            next_page_token: None,
        })
        .with_download("q3", b"budget\n".to_vec());
    let batch = read_rows(&client, &path, None).unwrap();
    assert_eq!(batch.rows.len(), 1);
    let content_idx = batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == "content")
        .expect("content column");
    assert_eq!(
        batch.rows[0].values[content_idx],
        Value::Bytes(b"budget\n".to_vec())
    );
    // The name reached Drive verbatim: the `?` is a character of the name, neither a glob nor a
    // `?query=` suffix, and the spaces did not truncate it.
    let queries: Vec<String> = client
        .recorded()
        .iter()
        .filter_map(|c| match c {
            RecordedCall::ListFiles { query, .. } => Some(query.clone()),
            _ => None,
        })
        .collect();
    assert!(
        queries[0].contains(&format!("name = '{NAME}'")),
        "the reserved characters survive into the lookup, got: {}",
        queries[0]
    );

    // REMOVE: the same path trashes exactly that file. It resolves to a FILE, so the fail-closed
    // folder guard (ticket 20260717102000) correctly does not fire.
    let client = MockDriveClient::new().with_list_page(FilePage {
        files: vec![file],
        next_page_token: None,
    });
    let resolver = crate::read::ClientResolver { client: &client };
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target(&path));
    match DriveEffect::from_node_with(&node, &resolver).unwrap() {
        DriveEffect::Trash { id } => assert_eq!(id, "q3", "only the addressed file is trashed"),
        other => panic!("expected Trash of the one matched file, got {other:?}"),
    }
}

#[test]
fn remove_defaults_to_trash_not_permanent_delete() {
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("id:f1"));
    assert!(node.irreversible, "REMOVE is inherently irreversible");
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Trash { id } => assert_eq!(id, "f1"),
        other => panic!("expected Trash (recoverable), got {other:?}"),
    }
    // An explicit hard_delete flag selects the permanent, irreversible delete.
    let hard = EffectNode::new(NodeId(1), EffectKind::Remove, target("id:f1"))
        .with_args(args(&[(HARD_DELETE_COL, Value::Bool(true))]));
    match DriveEffect::from_node(&hard).unwrap() {
        DriveEffect::Delete { id } => assert_eq!(id, "f1"),
        other => panic!("expected Delete, got {other:?}"),
    }
}

#[test]
fn call_drive_copy_decodes_and_unknown_proc_rejected() {
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("drive.copy")),
        target("id:f1"),
    )
    .with_args(args(&[
        (FILE_ID_COL, Value::Text("f1".into())),
        (PARENT_ID_COL, Value::Text("folder2".into())),
        (NAME_COL, Value::Text("copy.json".into())),
    ]));
    match DriveEffect::from_node(&node).unwrap() {
        DriveEffect::Copy { id, parent, name } => {
            assert_eq!(id, "f1");
            assert_eq!(parent, "folder2");
            assert_eq!(name, "copy.json");
        }
        other => panic!("expected Copy, got {other:?}"),
    }
    let bad = EffectNode::new(
        NodeId(1),
        EffectKind::Call(ProcId::new("drive.nuke")),
        target("id:f1"),
    );
    assert_eq!(
        DriveEffect::from_node(&bad).unwrap_err().code(),
        "unknown_procedure"
    );
}

/// `drive.copy(parent_path => …)` names the destination as a FOLDER PATH (the `cp`-parity form):
/// the apply leg walks it to the folder id live, instead of demanding an opaque `parent_id`.
#[tokio::test]
async fn copy_resolves_a_destination_folder_path_to_its_id() {
    use qfs_runtime::SharedApplier;
    let archive = FileMeta::for_test("arch1", "Archive", FOLDER_MIME, vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![archive])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Call(ProcId::new("drive.copy")),
        target("id:f1"),
    )
    .with_args(args(&[
        (FILE_ID_COL, Value::Text("f1".into())),
        (PARENT_PATH_COL, Value::Text("/drive/my/Archive".into())),
        (NAME_COL, Value::Text("backup.txt".into())),
    ]));
    driver.drive_applier().apply_shared(&node).unwrap();
    assert!(
        mock.recorded().into_iter().any(|c| matches!(&c,
            RecordedCall::CopyFile { id, parent, name }
                if id == "f1" && parent == "arch1" && name == "backup.txt")),
        "the walk resolved /drive/my/Archive → arch1 as the copy destination"
    );
}

// ---- PREVIEW performs no I/O (mock asserts zero calls) -----------------------------------

#[test]
fn preview_of_a_trash_plan_performs_no_io() {
    let (_d, mock) = driver_with_mock();
    let mut b = PlanBuilder::new();
    b.push(EffectNode::new(
        NodeId(0),
        EffectKind::Remove,
        target("id:f1"),
    ));
    let plan = b.build();
    let pv = preview(&plan);
    assert_eq!(pv.rows.len(), 1);
    assert!(
        pv.rows[0].irreversible,
        "preview surfaces the irreversible REMOVE (trash)"
    );
    assert!(
        mock.recorded().is_empty(),
        "PREVIEW must perform zero Drive API calls: {:?}",
        mock.recorded()
    );
}

// ---- token never in logs / errors --------------------------------------------------------

#[test]
fn errors_are_secret_free() {
    let errs = [
        DriveError::Api {
            op: "files.list",
            status: 500,
        },
        DriveError::CapabilityDenied {
            path: "id:f1".into(),
            verb: "INSERT",
        },
        DriveError::from(qfs_google_auth::AuthError::TokenRefresh {
            reason: "invalid_grant".to_string(),
        }),
    ];
    for e in &errs {
        let text = format!("{e} {e:?}");
        assert!(!text.contains("Bearer"), "no bearer in error: {text}");
        assert!(!text.contains("ya29"), "no google token prefix: {text}");
    }
    match DriveError::from(qfs_google_auth::AuthError::TokenRefresh {
        reason: "invalid_grant".to_string(),
    }) {
        DriveError::Auth { code, reauthorize } => {
            assert_eq!(code, "auth_token_refresh");
            assert!(reauthorize);
        }
        other => panic!("expected Auth, got {other:?}"),
    }
}

// ---- end-to-end: commit through interpreter + bridge -------------------------------------

#[tokio::test]
async fn commit_trash_file_end_to_end_through_interpreter() {
    let (driver, mock) = driver_with_mock();
    let bridge = gdrive_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(EffectNode::new(
        NodeId(0),
        EffectKind::Remove,
        target("id:f1"),
    ));
    let plan = b.build();
    plan.validate().unwrap();

    let caps = CapabilitySet::none().grant(DriverId::new("drive"), &EffectKind::Remove);
    let outcome = interp.commit(plan, &caps).await.unwrap();

    assert!(outcome.is_complete(), "trash applied: {outcome:?}");
    assert_eq!(outcome.applied_ids(), vec![NodeId(0)]);
    // The applier trashed (not permanently deleted) exactly one file.
    assert_eq!(
        mock.recorded(),
        vec![RecordedCall::Trash {
            id: "f1".to_string()
        }]
    );
}

#[tokio::test]
async fn commit_upsert_replaces_content_by_id() {
    let (driver, mock) = driver_with_mock();
    let bridge = gdrive_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(NodeId(0), EffectKind::Upsert, target("id:f1")).with_args(args(&[
            (FILE_ID_COL, Value::Text("f1".into())),
            (MIME_COL, Value::Text("application/json".into())),
            (BYTES_COL, Value::Bytes(b"[]".to_vec())),
        ])),
    );
    let plan = b.build();

    let caps = CapabilitySet::none().grant(DriverId::new("drive"), &EffectKind::Upsert);
    let outcome = interp.commit(plan, &caps).await.unwrap();
    assert!(outcome.is_complete(), "upsert applied: {outcome:?}");
    assert_eq!(
        mock.recorded(),
        vec![RecordedCall::UpdateContent {
            id: "f1".to_string(),
            mime: "application/json".to_string(),
            len: 2,
        }]
    );
}

#[tokio::test]
async fn multi_account_selects_independent_clients() {
    // Two accounts → two independent driver instances over two independent mock clients. Each
    // driver routes only to its own client; selection is the t19 base (one client per account).
    let mock_a = Arc::new(MockDriveClient::new());
    let mock_b = Arc::new(MockDriveClient::new());
    let driver_a = GDriveDriver::new(mock_a.clone() as Arc<dyn GDriveClient>);
    let driver_b = GDriveDriver::new(mock_b.clone() as Arc<dyn GDriveClient>);

    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("id:fA"));
    use qfs_runtime::SharedApplier;
    driver_a.drive_applier().apply_shared(&node).unwrap();

    assert_eq!(
        mock_a.recorded(),
        vec![RecordedCall::Trash {
            id: "fA".to_string()
        }]
    );
    assert!(
        mock_b.recorded().is_empty(),
        "account B's client was untouched"
    );
    let _ = driver_b;
}

// ---- apply-time path resolution (the write walk, 20260703150000) --------------------------
//
// A path-addressed write carries no snapshotted `parent_id`/`file_id` (those exist only for
// effects born from a scan), so the applier resolves them live through the SAME name→id walk
// the read path uses. These drive the applier end-to-end over the mock client.

fn walk_page(files: Vec<FileMeta>) -> FilePage {
    FilePage {
        files,
        next_page_token: None,
    }
}

/// gdrive-ftp `put` at the top level: an UPSERT at `/drive/my/<file>` with no id columns
/// resolves the parent to Drive's reserved `root` alias (after the existing-file probe finds
/// nothing) and uploads under the path's leaf name.
#[tokio::test]
async fn root_level_path_upload_resolves_parent_to_the_root_alias() {
    use qfs_runtime::SharedApplier;
    // One empty walk page: the existing-file probe for `hello.txt` under root finds nothing.
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Upsert, target("/drive/my/hello.txt"))
        .with_args(args(&[(BYTES_COL, Value::Bytes(b"hi qfs".to_vec()))]));
    driver.drive_applier().apply_shared(&node).unwrap();
    let uploaded = mock.recorded().into_iter().any(|c| {
        matches!(&c, RecordedCall::Upload { parent, name, .. }
            if parent == "root" && name == "hello.txt")
    });
    assert!(
        uploaded,
        "the upload lands under the root alias with the leaf name"
    );
}

/// gdrive-ftp `mkdir` at the top level: an INSERT at `/drive/my` carrying (name, folder-mime)
/// resolves the corpus root to the `root` alias with NO parent walk. Create-only (ticket
/// 20260708000100) adds one leaf-existence probe under the root before the byteless folder create.
#[tokio::test]
async fn mkdir_at_the_my_drive_root_needs_no_parent_walk() {
    use qfs_runtime::SharedApplier;
    let mock = Arc::new(MockDriveClient::new());
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node =
        EffectNode::new(NodeId(0), EffectKind::Insert, target("/drive/my")).with_args(args(&[
            (NAME_COL, Value::Text("qfs-plugin-test".into())),
            (MIME_COL, Value::Text(FOLDER_MIME.to_string())),
        ]));
    driver.drive_applier().apply_shared(&node).unwrap();
    assert_eq!(
        mock.recorded(),
        vec![
            // Create-only probe: is `qfs-plugin-test` already a child of the root alias?
            RecordedCall::ListFiles {
                query: "name = 'qfs-plugin-test' and 'root' in parents and trashed = false"
                    .to_string(),
                drive_id: None,
                page_size: Some(2),
            },
            // The name is free → the byteless folder create under the root alias (no parent walk).
            RecordedCall::Upload {
                parent: "root".to_string(),
                name: "qfs-plugin-test".to_string(),
                mime: FOLDER_MIME.to_string(),
                len: 0,
            },
        ],
        "create-only INSERT probes the leaf under the root alias, then creates"
    );
}

/// A nested path upload walks the intermediate folders name-by-name to the parent id.
#[tokio::test]
async fn nested_path_upload_walks_the_parent_folders() {
    use qfs_runtime::SharedApplier;
    let reports = FileMeta::for_test("rep1", "Reports", FOLDER_MIME, vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![reports])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        target("/drive/my/Reports/x.txt"),
    )
    .with_args(args(&[(BYTES_COL, Value::Bytes(b"hi".to_vec()))]));
    driver.drive_applier().apply_shared(&node).unwrap();
    let uploaded = mock.recorded().into_iter().any(|c| {
        matches!(&c, RecordedCall::Upload { parent, name, .. }
            if parent == "rep1" && name == "x.txt")
    });
    assert!(uploaded, "the walk resolved Reports → rep1 as the parent");
}

/// UPSERT convergence: when the path already names a file, the write becomes a content
/// replace by the resolved id (retry-safe), never a duplicate upload.
#[tokio::test]
async fn path_addressed_upsert_converges_to_a_content_replace() {
    use qfs_runtime::SharedApplier;
    let existing = FileMeta::for_test("f9", "x.txt", "text/plain", vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![existing])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Upsert, target("/drive/my/x.txt"))
        .with_args(args(&[(BYTES_COL, Value::Bytes(b"v2".to_vec()))]));
    driver.drive_applier().apply_shared(&node).unwrap();
    let replaced = mock
        .recorded()
        .into_iter()
        .any(|c| matches!(&c, RecordedCall::UpdateContent { id, .. } if id == "f9"));
    assert!(
        replaced,
        "the existing file is replaced by id, not re-uploaded"
    );
}

/// Create-only INSERT (ticket 20260708000100): when the target name already resolves to a Drive
/// file, the write REFUSES rather than duplicating or replacing it — the guard against a silent
/// overwrite on an inferred copy. Nothing is uploaded; UPSERT remains the explicit replace verb.
#[tokio::test]
async fn insert_onto_an_existing_name_refuses() {
    use qfs_runtime::SharedApplier;
    let existing = FileMeta::for_test("f9", "x.txt", "text/plain", vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![existing])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/drive/my/x.txt"))
        .with_args(args(&[(BYTES_COL, Value::Bytes(b"v1".to_vec()))]));
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    assert!(
        !mock
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::Upload { .. })),
        "a create-only INSERT onto an existing name uploads nothing"
    );
    let msg = format!("{err:?}");
    assert!(
        msg.contains("already exists"),
        "the refusal names the existing target: {msg}"
    );
}

/// A path segment that resolves to more than one same-named Drive file is refused as ambiguous
/// (ticket 20260708000100) — Drive names are not unique, so the driver never guesses which one.
#[tokio::test]
async fn a_name_matching_two_files_is_refused_as_ambiguous() {
    use qfs_runtime::SharedApplier;
    let a = FileMeta::for_test("fa", "dup.txt", "text/plain", vec!["root".to_string()]);
    let b = FileMeta::for_test("fb", "dup.txt", "text/plain", vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![a, b])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    // Resolving the path (here via a path-addressed REMOVE) hits the ambiguous match and refuses.
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/my/dup.txt"));
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.to_lowercase().contains("ambiguous"),
        "the resolve refuses on ambiguity instead of guessing: {msg}"
    );
    assert!(
        !mock
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::Trash { .. } | RecordedCall::Delete { .. })),
        "an ambiguous target trashes nothing"
    );
}

/// A path-addressed REMOVE resolves the file id through the walk and trashes it.
#[tokio::test]
async fn path_addressed_remove_resolves_and_trashes() {
    use qfs_runtime::SharedApplier;
    let existing = FileMeta::for_test("f7", "old.txt", "text/plain", vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![existing])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/my/old.txt"));
    driver.drive_applier().apply_shared(&node).unwrap();
    assert_eq!(
        mock.recorded().last(),
        Some(&RecordedCall::Trash {
            id: "f7".to_string()
        })
    );
}

/// Set-wide REMOVE honesty: a single `name` filter key trashes THAT child; a bare no-filter
/// remove of a folder path trashes the folder itself; a richer filter fails closed instead of
/// trashing the wrong node; and the bare corpus root can never be trashed.
#[tokio::test]
async fn set_wide_remove_resolves_by_name_or_fails_closed() {
    use qfs_runtime::SharedApplier;

    // name-only filter under the root: resolve the child and trash it. The filter rides the
    // WHERE-SELECTOR (§7) — a REMOVE writes nothing, so its `args` stays empty.
    let child = FileMeta::for_test("f5", "old.txt", "text/plain", vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![child])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/my"))
        .with_selector(args(&[(NAME_COL, Value::Text("old.txt".into()))]));
    driver.drive_applier().apply_shared(&node).unwrap();
    assert_eq!(
        mock.recorded().last(),
        Some(&RecordedCall::Trash {
            id: "f5".to_string()
        })
    );

    // A richer filter (name + another key) fails closed — never a folder-wide trash.
    let mock = Arc::new(MockDriveClient::new());
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node =
        EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/my")).with_selector(args(&[
            (NAME_COL, Value::Text("old.txt".into())),
            ("size", Value::Int(0)),
        ]));
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    assert!(
        format!("{err:?}").contains("REMOVE"),
        "fails closed: {err:?}"
    );
    assert!(mock.recorded().is_empty(), "nothing was trashed");

    // The bare corpus root resolves to nothing — the root itself can never be trashed.
    let mock = Arc::new(MockDriveClient::new());
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/my"));
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    assert!(
        format!("{err:?}").contains("nothing to remove"),
        "bare root remove fails closed: {err:?}"
    );
    assert!(mock.recorded().is_empty());
}

/// The 2026-07-17 incident, verbatim (ticket 20260717102000): `remove
/// /drive/shared/<Drive>/<folder> where name == '<file>'` committed through the REAL runtime
/// seam (interpreter → `EffectInput` → bridge) must trash ONLY the WHERE-matched child. The
/// live round trashed the folder node itself (with ~30 files) because `EffectInput` dropped
/// the plan node's WHERE-selector, so the applier decoded a bare folder REMOVE.
#[tokio::test]
async fn shared_drive_remove_where_survives_the_runtime_seam_and_trashes_only_the_match() {
    use crate::schema::SharedDrive;

    // A Shared Drive "Team" holding folder Reports (folder9) with a child spreadsheet (x1).
    let reports = FileMeta {
        drive_id: "drv1".to_string(),
        ..FileMeta::for_test("folder9", "Reports", FOLDER_MIME, vec!["drv1".to_string()])
    };
    let sheet = FileMeta {
        drive_id: "drv1".to_string(),
        ..FileMeta::for_test(
            "x1",
            "budget 2026.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            vec!["folder9".to_string()],
        )
    };
    let mock = Arc::new(
        MockDriveClient::new()
            .with_drive(SharedDrive::for_test("drv1", "Team"))
            // The child-path walk: resolve "Reports" under the drive root, then the sheet under it.
            .with_list_page(walk_page(vec![reports]))
            .with_list_page(walk_page(vec![sheet])),
    );
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let bridge = gdrive_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Remove,
            target("/drive/shared/Team/Reports"),
        )
        .with_selector(args(&[(NAME_COL, Value::Text("budget 2026.xlsx".into()))])),
    );
    let plan = b.build();
    plan.validate().unwrap();

    let caps = CapabilitySet::none().grant(DriverId::new("drive"), &EffectKind::Remove);
    let outcome = interp.commit(plan, &caps).await.unwrap();
    assert!(outcome.is_complete(), "trash applied: {outcome:?}");

    // Exactly the matched child was trashed — NEVER the folder node.
    let trashed: Vec<String> = mock
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            RecordedCall::Trash { id } => Some(id),
            RecordedCall::Delete { id } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(
        trashed,
        vec!["x1".to_string()],
        "only the WHERE-matched file is trashed"
    );
}

/// Fail-closed (ticket 20260717102000): a bare name-path REMOVE that resolves to a FOLDER is
/// refused — whole-folder trashing requires the explicit id-addressed form. Without this guard
/// a lost/unresolvable selector silently widens to the folder and its entire subtree.
#[tokio::test]
async fn bare_folder_path_remove_is_refused_toward_the_explicit_id_form() {
    use qfs_runtime::SharedApplier;
    let folder = FileMeta::for_test("fold1", "Reports", FOLDER_MIME, vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![folder])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/my/Reports"));
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("FOLDER") && msg.contains("id:fold1"),
        "refuses and points at the explicit id-addressed form: {msg}"
    );
    assert!(
        !mock
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::Trash { .. } | RecordedCall::Delete { .. })),
        "nothing was trashed"
    );

    // The explicit distinct form stays available: `remove /drive/id:<folder-id>` trashes the
    // folder (with its subtree) deliberately.
    let mock = Arc::new(MockDriveClient::new());
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("id:fold1"));
    driver.drive_applier().apply_shared(&node).unwrap();
    assert_eq!(
        mock.recorded(),
        vec![RecordedCall::Trash {
            id: "fold1".to_string()
        }]
    );
}

// ---- multi-row writes (ticket 20260712005000: honest counts on the write side) ------------

/// Build a multi-row args batch sharing one schema (the shape a routed partition or any
/// pipeline source with several rows hands the effect).
fn multi_args(cols: &[&str], rows: &[&[Value]]) -> RowBatch {
    let schema = Schema::new(
        cols.iter()
            .enumerate()
            .map(|(i, n)| Column::new(*n, rows[0][i].type_of(), true))
            .collect(),
    );
    RowBatch::new(schema, rows.iter().map(|r| Row::new(r.to_vec())).collect())
}

/// The live-round bug verbatim: a 2-row folder INSERT must create BOTH files and report
/// `affected == 2` — previously it uploaded only the first row while the summary claimed 2.
/// The text payloads also lock the text→bytes coercion the live round proved (a routed subject
/// string lands as the file's content bytes).
#[tokio::test]
async fn multi_row_folder_insert_uploads_every_row_and_counts_honestly() {
    use qfs_runtime::SharedApplier;
    let mock = Arc::new(MockDriveClient::new());
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/drive/my/reports"))
        .with_args(multi_args(
            &[PARENT_ID_COL, NAME_COL, BYTES_COL],
            &[
                &[
                    Value::Text("folder1".into()),
                    Value::Text("a.txt".into()),
                    Value::Text("one".into()),
                ],
                &[
                    Value::Text("folder1".into()),
                    Value::Text("b.txt".into()),
                    Value::Text("two!".into()),
                ],
            ],
        ));
    let out = driver.drive_applier().apply_shared(&node).unwrap();
    assert_eq!(out.affected, 2, "affected equals files actually created");
    let uploads: Vec<(String, usize)> = mock
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            RecordedCall::Upload { name, len, .. } => Some((name, len)),
            _ => None,
        })
        .collect();
    assert_eq!(
        uploads,
        vec![("a.txt".to_string(), 3), ("b.txt".to_string(), 4)],
        "one upload per row, in row order, text coerced to its UTF-8 bytes"
    );
}

/// A path-addressed multi-row INSERT resolves the shared destination folder ONCE (the memoized
/// walk) and still uploads every row. One queued walk page proves it: if each row re-walked,
/// the second walk would pop an empty page and fail to resolve.
#[tokio::test]
async fn multi_row_path_addressed_insert_walks_the_parent_once() {
    use qfs_runtime::SharedApplier;
    let reports = FileMeta::for_test("rep1", "Reports", FOLDER_MIME, vec!["root".to_string()]);
    let mock = Arc::new(MockDriveClient::new().with_list_page(walk_page(vec![reports])));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/drive/my/Reports"))
        .with_args(multi_args(
            &[NAME_COL, BYTES_COL],
            &[
                &[Value::Text("a.txt".into()), Value::Bytes(b"one".to_vec())],
                &[Value::Text("b.txt".into()), Value::Bytes(b"two".to_vec())],
            ],
        ));
    let out = driver.drive_applier().apply_shared(&node).unwrap();
    assert_eq!(out.affected, 2);
    let parents: Vec<String> = mock
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            RecordedCall::Upload { parent, .. } => Some(parent),
            _ => None,
        })
        .collect();
    assert_eq!(
        parents,
        vec!["rep1".to_string(), "rep1".to_string()],
        "both rows landed under the once-resolved parent"
    );
}

/// A mid-batch API failure must NOT report full success: the error names exactly how far the
/// batch got (`partial_apply`), and no later row is attempted after the failure.
#[tokio::test]
async fn multi_row_mid_batch_failure_reports_partial_progress() {
    use qfs_runtime::SharedApplier;
    let mock = Arc::new(MockDriveClient::new().with_upload_capacity(1));
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let rows: Vec<Vec<Value>> = (0..3)
        .map(|i| {
            vec![
                Value::Text("folder1".into()),
                Value::Text(format!("f{i}.txt")),
                Value::Bytes(b"x".to_vec()),
            ]
        })
        .collect();
    let row_refs: Vec<&[Value]> = rows.iter().map(Vec::as_slice).collect();
    let node = EffectNode::new(NodeId(0), EffectKind::Insert, target("/drive/my/reports"))
        .with_args(multi_args(&[PARENT_ID_COL, NAME_COL, BYTES_COL], &row_refs));
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    let text = format!("{err:?}");
    assert!(
        text.contains("row 1 of 3") && text.contains("1 file(s)"),
        "the error reports exact progress, never full success: {text}"
    );
    let uploads = mock
        .recorded()
        .into_iter()
        .filter(|c| matches!(c, RecordedCall::Upload { .. }))
        .count();
    assert_eq!(uploads, 1, "rows after the failed one are not attempted");
}

/// The id-addressed kinds target one node per statement: a multi-row REMOVE fails closed
/// instead of silently dropping the extra rows (or trashing more than consented).
#[tokio::test]
async fn multi_row_remove_fails_closed() {
    use qfs_runtime::SharedApplier;
    let mock = Arc::new(MockDriveClient::new());
    let driver = GDriveDriver::new(mock.clone() as Arc<dyn GDriveClient>);
    let node = EffectNode::new(NodeId(0), EffectKind::Remove, target("/drive/id:f1")).with_args(
        multi_args(
            &[NAME_COL],
            &[
                &[Value::Text("a.txt".into())],
                &[Value::Text("b.txt".into())],
            ],
        ),
    );
    let err = driver.drive_applier().apply_shared(&node).unwrap_err();
    assert!(
        format!("{err:?}").contains("single-target"),
        "multi-row REMOVE is refused: {err:?}"
    );
    assert!(mock.recorded().is_empty(), "nothing was trashed");
}
