//! Unit + differential tests for the local combine engine (t14, ADR-0002).
//!
//! Covers each residual operator (filter, project, sort, distinct, limit, group/
//! aggregate, expand), cross-source hash-join + set-op federation, and the **differential
//! property**: executing a partitioned plan over in-memory scan fakes returns the same
//! rows a naive all-local evaluation would. All scan results are in-memory (no network).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qfs_driver::PushdownProfile;
use qfs_engine::{CombineEngine, MiniEvaluator, ScanResults};
use qfs_pushdown::{
    partition_by_source, Aggregate, Aggregator, JoinKind, JoinOn, LogicalPlan, PhysicalPlan,
    ScalarExpr, SetKind, SourceId, SourceRegistry,
};
use qfs_types::{
    CmpOp, ColRef, Column, ColumnType, Fields, Literal, Predicate, Row, RowBatch, Schema,
    TransformMode, Value,
};

fn users_schema() -> Schema {
    Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new("name", ColumnType::Text, true),
        Column::new("age", ColumnType::Int, true),
    ])
}

fn users_batch() -> RowBatch {
    RowBatch::new(
        users_schema(),
        vec![
            Row::new(vec![
                Value::Int(1),
                Value::Text("ann".into()),
                Value::Int(40),
            ]),
            Row::new(vec![
                Value::Int(2),
                Value::Text("bob".into()),
                Value::Int(25),
            ]),
            Row::new(vec![
                Value::Int(3),
                Value::Text("cy".into()),
                Value::Int(35),
            ]),
        ],
    )
}

fn none() -> PushdownProfile {
    PushdownProfile::None
}

// ---- Residual filter correctness (None source ⇒ engine filters locally) ----

fn classify_plan() -> LogicalPlan {
    LogicalPlan::Transform {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), users_schema())),
        name: "classify".into(),
        output_schema: Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
        mode: TransformMode::RowWise,
    }
}

#[test]
fn a_transform_stage_fails_closed_with_no_executor() {
    // §15 (decision W): the plan spine builds a well-formed Transform node (forced local). Without
    // an injected executor (the read/preview shape) the engine fails CLOSED — a structured error,
    // never silent no-op rows. Only the COMMIT boundary injects the executor.
    let reg = SourceRegistry::new().with(SourceId::new("api"), PushdownProfile::None);
    let phys = partition_by_source(&classify_plan(), &reg).unwrap();
    let err = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![users_batch()]))
        .unwrap_err();
    assert_eq!(err.code(), "transform_no_executor");
}

/// A deterministic mock executor: emits one `label` = "L:<name>" row per input row (row-wise), so
/// the engine's OUTPUT-membership + row plumbing can be proven without any model call.
struct MockExec;
impl qfs_engine::TransformExecutor for MockExec {
    fn execute(
        &self,
        call: &qfs_engine::TransformCall<'_>,
        input: RowBatch,
    ) -> Result<RowBatch, String> {
        let rows = input
            .rows
            .iter()
            .map(|_| Row::new(vec![Value::Text(format!("L:{}", call.name))]))
            .collect();
        Ok(RowBatch::new(
            Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
            rows,
        ))
    }
}

#[test]
fn a_transform_stage_runs_through_the_injected_executor() {
    // With the executor injected (the COMMIT shape), the model stage runs: three input rows in,
    // three OUTPUT rows out, carrying the declared OUTPUT schema.
    let reg = SourceRegistry::new().with(SourceId::new("api"), PushdownProfile::None);
    let phys = partition_by_source(&classify_plan(), &reg).unwrap();
    let out = MiniEvaluator::with_transform(std::sync::Arc::new(MockExec))
        .execute(&phys, ScanResults::new(vec![users_batch()]))
        .unwrap();
    assert_eq!(out.schema.column_names(), vec!["label"]);
    assert_eq!(out.rows.len(), 3);
    assert_eq!(out.rows[0].values[0], Value::Text("L:classify".into()));
}

/// A mock that returns a column the definition's OUTPUT never declared — the untrusted-output
/// case the engine must reject.
struct BadOutputExec;
impl qfs_engine::TransformExecutor for BadOutputExec {
    fn execute(
        &self,
        _call: &qfs_engine::TransformCall<'_>,
        _input: RowBatch,
    ) -> Result<RowBatch, String> {
        Ok(RowBatch::new(
            Schema::new(vec![Column::new("wrong", ColumnType::Text, true)]),
            vec![Row::new(vec![Value::Text("x".into())])],
        ))
    }
}

#[test]
fn a_transform_output_violation_is_a_structured_error() {
    let reg = SourceRegistry::new().with(SourceId::new("api"), PushdownProfile::None);
    let phys = partition_by_source(&classify_plan(), &reg).unwrap();
    let err = MiniEvaluator::with_transform(std::sync::Arc::new(BadOutputExec))
        .execute(&phys, ScanResults::new(vec![users_batch()]))
        .unwrap_err();
    assert_eq!(err.code(), "transform_output_mismatch");
}

#[test]
fn residual_filter_runs_locally() {
    // WHERE age > 30 over a None source: the planner leaves the filter local; the engine
    // applies it. Expect ann(40) and cy(35).
    let plan = LogicalPlan::Filter {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), users_schema())),
        predicate: Predicate::Cmp(ColRef::col("age"), CmpOp::Gt, Literal::Int(30)),
    };
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();

    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![users_batch()]))
        .unwrap();
    assert_eq!(out.rows.len(), 2);
    let names: Vec<&str> = out
        .rows
        .iter()
        .map(|r| match &r.values[1] {
            Value::Text(s) => s.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(names, vec!["ann", "cy"]);
}

#[test]
fn residual_project_sort_limit_distinct() {
    // SELECT name |> ORDER BY name |> LIMIT 2 over a None source.
    let plan = LogicalPlan::Limit {
        input: Box::new(LogicalPlan::Sort {
            input: Box::new(LogicalPlan::Project {
                input: Box::new(LogicalPlan::scan(SourceId::new("api"), users_schema())),
                columns: vec!["name".into()],
            }),
            keys: vec![qfs_pushdown::OrderKey {
                column: "name".into(),
                descending: false,
            }],
        }),
        n: 2,
    };
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![users_batch()]))
        .unwrap();
    assert_eq!(out.schema.column_names(), vec!["name"]);
    let names: Vec<&str> = out
        .rows
        .iter()
        .map(|r| match &r.values[0] {
            Value::Text(s) => s.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(names, vec!["ann", "bob"]); // sorted asc, first two
}

// ---- Group/aggregate ----

#[test]
fn residual_group_aggregate_count_and_sum() {
    // Two departments; COUNT and SUM of salary per dept.
    let schema = Schema::new(vec![
        Column::new("dept", ColumnType::Text, false),
        Column::new("salary", ColumnType::Int, false),
    ]);
    let batch = RowBatch::new(
        schema.clone(),
        vec![
            Row::new(vec![Value::Text("eng".into()), Value::Int(100)]),
            Row::new(vec![Value::Text("eng".into()), Value::Int(200)]),
            Row::new(vec![Value::Text("ops".into()), Value::Int(50)]),
        ],
    );
    let plan = LogicalPlan::Aggregate {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), schema)),
        group_by: vec!["dept".into()],
        aggregates: vec![
            Aggregate {
                func: Aggregator::Count,
                column: "salary".into(),
                output: "n".into(),
            },
            Aggregate {
                func: Aggregator::Sum,
                column: "salary".into(),
                output: "total".into(),
            },
        ],
    };
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![batch]))
        .unwrap();
    assert_eq!(out.schema.column_names(), vec!["dept", "n", "total"]);
    // eng: count 2, sum 300; ops: count 1, sum 50.
    let mut rows: Vec<(String, i64, i64)> = out
        .rows
        .iter()
        .map(|r| {
            let dept = match &r.values[0] {
                Value::Text(s) => s.clone(),
                _ => String::new(),
            };
            let n = match &r.values[1] {
                Value::Int(n) => *n,
                _ => -1,
            };
            let t = match &r.values[2] {
                Value::Int(n) => *n,
                _ => -1,
            };
            (dept, n, t)
        })
        .collect();
    rows.sort();
    assert_eq!(rows, vec![("eng".into(), 2, 300), ("ops".into(), 1, 50)]);
}

// ---- EXPAND ----

#[test]
fn residual_expand_explodes_array_of_struct() {
    // One row with a `tags` array of {tag} structs → two rows.
    let inner = Schema::new(vec![Column::new("tag", ColumnType::Text, false)]);
    let schema = Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new(
            "tags",
            ColumnType::Array(Box::new(ColumnType::Struct(inner))),
            false,
        ),
    ]);
    let row = Row::new(vec![
        Value::Int(1),
        Value::Array(vec![
            Value::Struct(Fields::new(vec![("tag".into(), Value::Text("x".into()))])),
            Value::Struct(Fields::new(vec![("tag".into(), Value::Text("y".into()))])),
        ]),
    ]);
    let batch = RowBatch::new(schema.clone(), vec![row]);
    let plan = LogicalPlan::Expand {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), schema)),
        field: "tags".into(),
    };
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![batch]))
        .unwrap();
    assert_eq!(out.rows.len(), 2);
}

#[test]
fn residual_expand_splices_a_ragged_array_by_field_name() {
    // Ticket 20260725103000. Elements with DIFFERENT key sets — what every real optional-field API
    // returns. Splicing positionally shifted every later column silently; each element's fields must
    // land under their own names, and a key an element omits must be `Null` in that element's row.
    let inner = Schema::new(vec![
        Column::new("ts", ColumnType::Text, false),
        Column::new("user", ColumnType::Text, true),
        Column::new("subtype", ColumnType::Text, true),
    ]);
    let schema = Schema::new(vec![
        Column::new("ok", ColumnType::Bool, false),
        Column::new(
            "messages",
            ColumnType::Array(Box::new(ColumnType::Struct(inner))),
            false,
        ),
    ]);
    let row = Row::new(vec![
        Value::Bool(true),
        Value::Array(vec![
            // Fully populated, and deliberately NOT in the declared column order.
            Value::Struct(Fields::new(vec![
                ("subtype".into(), Value::Text("bot_message".into())),
                ("ts".into(), Value::Text("1".into())),
                ("user".into(), Value::Text("U1".into())),
            ])),
            // Omits `subtype` entirely — the ragged case.
            Value::Struct(Fields::new(vec![
                ("ts".into(), Value::Text("2".into())),
                ("user".into(), Value::Text("U2".into())),
            ])),
            // Carries `subtype` present-but-EMPTY, which is a different fact from omitting it.
            Value::Struct(Fields::new(vec![
                ("ts".into(), Value::Text("3".into())),
                ("user".into(), Value::Text("U3".into())),
                ("subtype".into(), Value::Text(String::new())),
            ])),
        ]),
    ]);
    let batch = RowBatch::new(schema.clone(), vec![row]);
    let plan = LogicalPlan::Expand {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), schema)),
        field: "messages".into(),
    };
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![batch]))
        .unwrap();

    assert_eq!(
        out.schema.column_names(),
        vec!["ok", "ts", "user", "subtype"],
        "the declared element type supplies the replacement columns, in its own order"
    );
    assert_eq!(
        out.rows
            .iter()
            .map(|r| r.values.clone())
            .collect::<Vec<_>>(),
        vec![
            vec![
                Value::Bool(true),
                Value::Text("1".into()),
                Value::Text("U1".into()),
                Value::Text("bot_message".into()),
            ],
            vec![
                Value::Bool(true),
                Value::Text("2".into()),
                Value::Text("U2".into()),
                // Omitted by this element — not the next column's value slid over.
                Value::Null,
            ],
            vec![
                Value::Bool(true),
                Value::Text("3".into()),
                Value::Text("U3".into()),
                // Present-but-empty stays empty: absent and "" are different wire facts.
                Value::Text(String::new()),
            ],
        ]
    );
}

#[test]
fn residual_expand_of_a_late_bound_column_takes_the_union_of_observed_fields() {
    // No declared element type (an `Unknown` column — a decoded body, an EXTEND output). The
    // replacement columns are the UNION of the field names the rows actually carry, in first-seen
    // order, so a ragged late-bound array is spliced by name too.
    let schema = Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new("items", ColumnType::Unknown, true),
    ]);
    let rows = vec![
        Row::new(vec![
            Value::Int(1),
            Value::Array(vec![Value::Struct(Fields::new(vec![
                ("a".into(), Value::Int(10)),
                ("b".into(), Value::Int(20)),
            ]))]),
        ]),
        Row::new(vec![
            Value::Int(2),
            // Introduces `c` and omits `b`.
            Value::Array(vec![Value::Struct(Fields::new(vec![
                ("a".into(), Value::Int(30)),
                ("c".into(), Value::Int(40)),
            ]))]),
        ]),
    ];
    let batch = RowBatch::new(schema.clone(), rows);
    let plan = LogicalPlan::Expand {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), schema)),
        field: "items".into(),
    };
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![batch]))
        .unwrap();

    assert_eq!(out.schema.column_names(), vec!["id", "a", "b", "c"]);
    assert_eq!(
        out.rows
            .iter()
            .map(|r| r.values.clone())
            .collect::<Vec<_>>(),
        vec![
            vec![Value::Int(1), Value::Int(10), Value::Int(20), Value::Null],
            vec![Value::Int(2), Value::Int(30), Value::Null, Value::Int(40)],
        ]
    );
}

// ---- Cross-source federation: hash join over two in-memory scans ----

#[test]
fn cross_source_hash_join_federates() {
    // pg users JOIN git authors ON id = id; each side a separate in-memory fake.
    let pg = users_batch();
    let git = RowBatch::new(
        Schema::new(vec![
            Column::new("id", ColumnType::Int, false),
            Column::new("sha", ColumnType::Text, false),
        ]),
        vec![
            Row::new(vec![Value::Int(1), Value::Text("abc".into())]),
            Row::new(vec![Value::Int(3), Value::Text("def".into())]),
        ],
    );
    let plan = LogicalPlan::Join {
        kind: JoinKind::Inner,
        lhs: Box::new(LogicalPlan::scan(SourceId::new("pg"), users_schema())),
        rhs: Box::new(LogicalPlan::scan(
            SourceId::new("git"),
            Schema::new(vec![
                Column::new("id", ColumnType::Int, false),
                Column::new("sha", ColumnType::Text, false),
            ]),
        )),
        on: JoinOn::eq("id", "id"),
    };
    let reg = SourceRegistry::new()
        .with(SourceId::new("pg"), none())
        .with(SourceId::new("git"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    // Scans are consumed left-to-right: pg first, then git.
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![pg, git]))
        .unwrap();
    // ids 1 and 3 join; id 2 (bob) has no git row.
    assert_eq!(out.rows.len(), 2);
    // The join schema disambiguates the colliding `id` column from the right side. The
    // right `id` has no provenance here, so it falls back to the positional `r.id`
    // qualifier (Schema::join policy) — no silent shadowing of the left `id`.
    assert!(out.schema.column("id").is_some());
    assert!(out.schema.column("r.id").is_some());
}

// ---- Set ops ----

#[test]
fn cross_source_union_except_intersect() {
    let s = Schema::new(vec![Column::new("v", ColumnType::Int, false)]);
    let mk = |xs: &[i64]| {
        RowBatch::new(
            s.clone(),
            xs.iter().map(|n| Row::new(vec![Value::Int(*n)])).collect(),
        )
    };
    for (kind, expect) in [
        (SetKind::Union, vec![1, 2, 3]),
        (SetKind::Except, vec![1]),
        (SetKind::Intersect, vec![2]),
    ] {
        let plan = LogicalPlan::SetOp {
            kind,
            lhs: Box::new(LogicalPlan::scan(SourceId::new("a"), s.clone())),
            rhs: Box::new(LogicalPlan::scan(SourceId::new("b"), s.clone())),
        };
        let reg = SourceRegistry::new()
            .with(SourceId::new("a"), none())
            .with(SourceId::new("b"), none());
        let phys = partition_by_source(&plan, &reg).unwrap();
        let out = MiniEvaluator::new()
            .execute(&phys, ScanResults::new(vec![mk(&[1, 2]), mk(&[2, 3])]))
            .unwrap();
        let got: Vec<i64> = out
            .rows
            .iter()
            .map(|r| match &r.values[0] {
                Value::Int(n) => *n,
                _ => -1,
            })
            .collect();
        assert_eq!(got, expect, "{kind:?}");
    }
}

// ---- Differential property: partitioned == all-local ----

/// A faithful "driver fake": pre-apply the scan's *pushed* work to the base batch, the
/// way a real backend would. This makes the differential honest — the partitioned run
/// pushes work to the (fake) driver, the all-local run does it in the engine, and both
/// must agree. The pushed predicate is applied by running it through the engine as a
/// one-node local Filter (re-using the engine's own predicate kernel), then the pushed
/// limit truncates — modelling exactly what a backend would do for this fixture.
fn run_scan(scan: &qfs_pushdown::ScanNode, base: RowBatch) -> RowBatch {
    let mut out = base;
    if let Some(p) = &scan.pushed.filter {
        let filter_plan = PhysicalPlan::Combine {
            op: qfs_pushdown::CombineOp::Filter(p.clone()),
            inputs: vec![PhysicalPlan::Scan(scan.clone())],
        };
        out = MiniEvaluator::new()
            .execute(&filter_plan, ScanResults::new(vec![out]))
            .unwrap();
    }
    if let Some(n) = scan.pushed.limit {
        out.rows.truncate(n as usize);
    }
    out
}

#[test]
fn differential_partitioned_equals_all_local() {
    // The SAME logical plan run two ways must yield the same rows (the t14 differential
    // property): (1) all-local — None source, the engine does everything; (2) partitioned
    // — a Partial source pushes WHERE+LIMIT (pre-applied by the driver fake) and leaves
    // SELECT local. Both must produce identical rows.
    let plan = LogicalPlan::Limit {
        input: Box::new(LogicalPlan::Project {
            input: Box::new(LogicalPlan::Filter {
                input: Box::new(LogicalPlan::scan(SourceId::new("db"), users_schema())),
                predicate: Predicate::Cmp(ColRef::col("age"), CmpOp::Ge, Literal::Int(30)),
            }),
            columns: vec!["name".into()],
        }),
        n: 10,
    };

    // (1) all-local ground truth: None source ⇒ the engine runs filter+project+limit.
    let local_reg = SourceRegistry::new().with(SourceId::new("db"), none());
    let local_phys = partition_by_source(&plan, &local_reg).unwrap();
    let local_out = MiniEvaluator::new()
        .execute(&local_phys, ScanResults::new(vec![users_batch()]))
        .unwrap();

    // (2) partitioned: a Partial source that pushes WHERE+LIMIT. The driver fake
    // pre-applies the pushed work to the base batch; the engine runs the residual SELECT.
    let partial = PushdownProfile::Partial {
        where_: true,
        project: false,
        limit: true,
        order: false,
        join: false,
        aggregate: false,
        distinct: false,
        group_by: false,
    };
    let part_reg = SourceRegistry::new().with(SourceId::new("db"), partial);
    let part_phys = partition_by_source(&plan, &part_reg).unwrap();
    // Pre-apply each scan's pushed work (one scan here).
    let scan_node = part_phys.scans()[0];
    let pushed_batch = run_scan(scan_node, users_batch());
    let part_out = MiniEvaluator::new()
        .execute(&part_phys, ScanResults::new(vec![pushed_batch]))
        .unwrap();

    // The two runs agree: both project `name` over age>=30 ⇒ ann(40), cy(35).
    let names = |b: &RowBatch| -> Vec<String> {
        b.rows
            .iter()
            .filter_map(|r| match &r.values[0] {
                Value::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(local_out.schema.column_names(), vec!["name"]);
    assert_eq!(part_out.schema.column_names(), vec!["name"]);
    assert_eq!(names(&local_out), vec!["ann".to_string(), "cy".to_string()]);
    assert_eq!(
        names(&part_out),
        names(&local_out),
        "partitioned execution must equal all-local"
    );
}

#[test]
fn missing_scan_result_is_a_structured_error() {
    let plan = LogicalPlan::scan(SourceId::new("api"), users_schema());
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let err = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![]))
        .unwrap_err();
    assert_eq!(err.code(), "missing_scan_result");
}

// ---- Cross-service pack: ProjectExpr + ARRAY_AGG + Extend (t92, ticket 192440) ----

fn drive_files_schema() -> Schema {
    Schema::new(vec![
        Column::new("name", ColumnType::Text, true),
        Column::new("mime_type", ColumnType::Text, true),
        Column::new("content", ColumnType::Bytes, true),
    ])
}

fn drive_files_batch() -> RowBatch {
    RowBatch::new(
        drive_files_schema(),
        vec![
            Row::new(vec![
                Value::Text("a.txt".into()),
                Value::Text("text/plain".into()),
                Value::Bytes(b"aaa".to_vec()),
            ]),
            Row::new(vec![
                Value::Text("b.pdf".into()),
                Value::Text("application/pdf".into()),
                Value::Bytes(b"bbb".to_vec()),
            ]),
        ],
    )
}

#[test]
fn cross_service_pack_attachments_project_expr_array_agg_extend() {
    // The 192440 composable recipe's read half, executed over an in-memory Drive scan:
    //   |> select {filename: name, mime: mime_type, bytes: content} as att   (ProjectExpr)
    //   |> aggregate array_agg(att) as attachments                           (ARRAY_AGG)
    //   |> extend to = 'a@x.y', subject = 'Q3', body = 'See attached'        (Extend)
    // Expect ONE row whose `attachments` is an Array of two Structs carrying each file's
    // bytes/filename/mime, plus the three extended draft columns.
    let att = ScalarExpr::Struct(vec![
        ("filename".into(), ScalarExpr::Col(ColRef::col("name"))),
        ("mime".into(), ScalarExpr::Col(ColRef::col("mime_type"))),
        ("bytes".into(), ScalarExpr::Col(ColRef::col("content"))),
    ]);
    let plan = LogicalPlan::Extend {
        input: Box::new(LogicalPlan::Aggregate {
            input: Box::new(LogicalPlan::ProjectExpr {
                input: Box::new(LogicalPlan::scan_at(
                    SourceId::new("drive"),
                    "/drive/my",
                    drive_files_schema(),
                )),
                projections: vec![("att".into(), att)],
            }),
            group_by: vec![],
            aggregates: vec![Aggregate {
                func: Aggregator::ArrayAgg,
                column: "att".into(),
                output: "attachments".into(),
            }],
        }),
        assignments: vec![
            ("to".into(), ScalarExpr::Lit(Value::Text("a@x.y".into()))),
            ("subject".into(), ScalarExpr::Lit(Value::Text("Q3".into()))),
            (
                "body".into(),
                ScalarExpr::Lit(Value::Text("See attached".into())),
            ),
        ],
    };

    let reg = SourceRegistry::new().with(SourceId::new("drive"), none());
    let phys = partition_by_source(&plan, &reg).unwrap();
    let out = MiniEvaluator::new()
        .execute(&phys, ScanResults::new(vec![drive_files_batch()]))
        .unwrap();

    assert_eq!(
        out.rows.len(),
        1,
        "array_agg collapses the two files into one row"
    );
    let row = &out.rows[0];
    let col = |name: &str| {
        out.schema
            .columns
            .iter()
            .position(|c| c.name == name)
            .unwrap()
    };

    let Value::Array(items) = &row.values[col("attachments")] else {
        panic!(
            "attachments must be an Array, got {:?}",
            row.values[col("attachments")]
        );
    };
    assert_eq!(items.len(), 2);
    let Value::Struct(f0) = &items[0] else {
        panic!("attachment 0 must be a Struct");
    };
    assert_eq!(f0.get("filename"), Some(&Value::Text("a.txt".into())));
    assert_eq!(f0.get("mime"), Some(&Value::Text("text/plain".into())));
    assert_eq!(f0.get("bytes"), Some(&Value::Bytes(b"aaa".to_vec())));
    let Value::Struct(f1) = &items[1] else {
        panic!("attachment 1 must be a Struct");
    };
    assert_eq!(f1.get("bytes"), Some(&Value::Bytes(b"bbb".to_vec())));

    assert_eq!(row.values[col("to")], Value::Text("a@x.y".into()));
    assert_eq!(row.values[col("subject")], Value::Text("Q3".into()));
    assert_eq!(row.values[col("body")], Value::Text("See attached".into()));
}

// ---- `WHERE` on an unknown column: a refusal, not an empty relation (ticket 20260717180100) ----

/// Run `plan` over `batch` through the real partition → evaluate path.
fn run_local(plan: &LogicalPlan, batch: RowBatch) -> Result<RowBatch, qfs_engine::EngineError> {
    let reg = SourceRegistry::new().with(SourceId::new("api"), none());
    let phys = partition_by_source(plan, &reg).unwrap();
    MiniEvaluator::new().execute(&phys, ScanResults::new(vec![batch]))
}

fn where_plan(schema: Schema, predicate: Predicate) -> LogicalPlan {
    LogicalPlan::Filter {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), schema)),
        predicate,
    }
}

#[test]
fn where_on_a_column_absent_from_a_described_schema_is_refused() {
    // The defect: `resolve` returns `None` for an absent column and the predicate maps that to
    // "the row does not match", so a TYPO returned the same empty relation an honest miss does —
    // at exit 0, indistinguishable in rows, schema and status. It is now a structured refusal
    // naming the column AND the schema it was checked against.
    let err = run_local(
        &where_plan(
            users_schema(),
            Predicate::Cmp(
                ColRef::col("nosuchcol"),
                CmpOp::Eq,
                Literal::Text("zzz".into()),
            ),
        ),
        users_batch(),
    )
    .expect_err("an unknown column is refused");
    assert_eq!(err.code(), "unknown_column");
    match &err {
        qfs_engine::EngineError::UnknownColumn {
            stage,
            name,
            available,
        } => {
            assert_eq!(*stage, "where");
            assert_eq!(name, "nosuchcol");
            assert_eq!(
                available,
                &vec!["id".to_string(), "name".into(), "age".into()]
            );
        }
        other => panic!("expected UnknownColumn, got {other:?}"),
    }
}

#[test]
fn where_on_a_real_column_with_no_match_stays_an_empty_relation_at_success() {
    // The control that makes the refusal meaningful: "nothing matched" must remain a valid answer,
    // not become an error. If this ever failed, the fix would have made every empty result look
    // like a malformed query — the opposite corruption.
    let out = run_local(
        &where_plan(
            users_schema(),
            Predicate::Cmp(
                ColRef::col("name"),
                CmpOp::Eq,
                Literal::Text("nobody".into()),
            ),
        ),
        users_batch(),
    )
    .expect("a real column with no match is not an error");
    assert!(out.rows.is_empty());
    assert_eq!(
        out.schema.column_names().len(),
        3,
        "the schema is preserved"
    );
}

#[test]
fn every_predicate_operator_refuses_an_unknown_column_not_just_cmp() {
    // `In`, `Between` and `Like` take the SAME `resolve → None → false` path `Cmp` does, so each
    // one hid a typo the same way; and a `NOT`/`OR` arm hides one just as well. All are checked.
    let unknown = ColRef::col("nosuchcol");
    let cases: Vec<(&str, Predicate)> = vec![
        (
            "in",
            Predicate::In(unknown.clone(), vec![Literal::Text("a".into())]),
        ),
        (
            "between",
            Predicate::Between(unknown.clone(), Literal::Int(1), Literal::Int(2)),
        ),
        (
            "like",
            Predicate::Like(unknown.clone(), qfs_types::Pattern("%x%".into())),
        ),
        (
            "not",
            Predicate::Not(Box::new(Predicate::Cmp(
                unknown.clone(),
                CmpOp::Eq,
                Literal::Text("x".into()),
            ))),
        ),
        (
            "or",
            Predicate::Or(
                Box::new(Predicate::Cmp(
                    ColRef::col("name"),
                    CmpOp::Eq,
                    Literal::Text("ann".into()),
                )),
                Box::new(Predicate::Cmp(
                    unknown.clone(),
                    CmpOp::Eq,
                    Literal::Text("x".into()),
                )),
            ),
        ),
    ];
    for (label, predicate) in cases {
        match run_local(&where_plan(users_schema(), predicate), users_batch()) {
            Err(e) => assert_eq!(e.code(), "unknown_column", "`{label}` refuses"),
            Ok(batch) => panic!("`{label}` returned rows instead of refusing: {batch:?}"),
        }
    }
}

#[test]
fn where_over_an_undescribable_schema_stays_late_bound() {
    // The leniency this fix must NOT remove: a driver that does not describe its columns yields an
    // EMPTY schema, and a predicate over it has to keep executing (the row values are the only
    // truth there). Refusing here would false-reject a column that really is present at runtime.
    let empty = Schema::new(Vec::new());
    let batch = RowBatch::new(empty.clone(), vec![Row::new(vec![])]);
    let out = run_local(
        &where_plan(
            empty,
            Predicate::Cmp(
                ColRef::col("whatever"),
                CmpOp::Eq,
                Literal::Text("x".into()),
            ),
        ),
        batch,
    )
    .expect("an undescribable relation is never refused");
    // The predicate still evaluates (and drops the row, since nothing resolves) — the point is
    // that it EXECUTED rather than being rejected at the seam.
    assert!(out.rows.is_empty());
}

#[test]
fn a_dotted_path_only_requires_its_head_column() {
    // `meta.title` navigates a Json/Struct value whose inner fields are late-bound by design, so
    // only `meta` must exist. Checking deeper would break the documented navigation semantics.
    let schema = Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new("meta", ColumnType::Json, true),
    ]);
    let batch = RowBatch::new(
        schema.clone(),
        vec![Row::new(vec![Value::Int(1), Value::Null])],
    );
    let out = run_local(
        &where_plan(
            schema.clone(),
            Predicate::Cmp(
                ColRef::path(vec!["meta".into(), "title".into()]),
                CmpOp::Eq,
                Literal::Text("x".into()),
            ),
        ),
        batch.clone(),
    );
    assert!(
        out.is_ok(),
        "a present head with a late-bound path executes"
    );

    // But an absent HEAD is still the malformed question this ticket is about.
    let err = run_local(
        &where_plan(
            schema,
            Predicate::Cmp(
                ColRef::path(vec!["nosuchcol".into(), "title".into()]),
                CmpOp::Eq,
                Literal::Text("x".into()),
            ),
        ),
        batch,
    )
    .expect_err("an absent head column is refused even under a dotted path");
    assert_eq!(err.code(), "unknown_column");
}

#[test]
fn a_driver_residual_may_name_a_backend_pseudo_column_and_still_evaluates() {
    // The counterpart the refusal must NOT catch: `apply_residual` carries a DRIVER's truthful
    // residual, which legitimately names a search pseudo-column the described schema does not have
    // (Drive's `fullText`, Gmail's `to`). Refusing there would turn every such `where` into an
    // error; the caller-written `WHERE` path (`apply_where`) is the one that refuses.
    let out = qfs_engine::apply_residual(
        users_batch(),
        &Predicate::Cmp(
            ColRef::col("fullText"),
            CmpOp::Eq,
            Literal::Text("x".into()),
        ),
    );
    assert!(out.rows.is_empty(), "it evaluates (and matches nothing)");

    let err = qfs_engine::apply_where(
        users_batch(),
        &Predicate::Cmp(
            ColRef::col("fullText"),
            CmpOp::Eq,
            Literal::Text("x".into()),
        ),
    )
    .expect_err("the caller-written form refuses the same predicate");
    assert_eq!(err.code(), "unknown_column");
}

// ---- The runtime twin of the planner's `SELECT` refusal (ticket 20260725113000) ----

/// A plan whose scan is **late-bound** (empty schema, so the planner stays lenient) with a
/// projection over it — the post-decode / declared-driver shape, where the only schema that exists
/// is the one the driver actually delivered.
fn late_bound_select_plan(columns: &[&str]) -> LogicalPlan {
    LogicalPlan::Project {
        input: Box::new(LogicalPlan::scan(SourceId::new("api"), Schema::empty())),
        columns: columns.iter().map(|c| (*c).to_string()).collect(),
    }
}

#[test]
fn select_on_a_column_absent_from_the_delivered_batch_is_refused() {
    // A projection reaches rows by two roads. When the relation was described, the planner refuses
    // before the scan; when it was late-bound (a decode, a declared driver with no `OF`), the
    // delivered batch is the first and only schema — so the refusal must exist here too, or the
    // undescribable case would keep answering rows of nothing at exit 0.
    let err = run_local(&late_bound_select_plan(&["nosuchcol"]), users_batch())
        .expect_err("an unknown column is refused over the delivered batch");
    assert_eq!(err.code(), "unknown_column");
    match &err {
        qfs_engine::EngineError::UnknownColumn {
            stage,
            name,
            available,
        } => {
            assert_eq!(*stage, "select");
            assert_eq!(name, "nosuchcol");
            assert_eq!(
                available,
                &vec!["id".to_string(), "name".into(), "age".into()]
            );
        }
        other => panic!("expected UnknownColumn, got {other:?}"),
    }
}

#[test]
fn select_over_a_delivered_batch_keeps_its_real_columns() {
    // The control: a projection naming real columns still narrows, in the order asked.
    let out = run_local(&late_bound_select_plan(&["name", "id"]), users_batch())
        .expect("a real projection is not an error");
    assert_eq!(
        out.schema.column_names(),
        vec!["name".to_string(), "id".into()]
    );
    assert_eq!(out.rows.len(), 3);
}

#[test]
fn a_projection_over_an_undescribed_empty_batch_stays_lenient() {
    // The leniency, at the runtime seam this time: a batch that carries no schema at all is
    // late-bound, not empty of the column — refusing it would break the undescribable relations
    // the same fold deliberately spares at plan time.
    let out = run_local(
        &late_bound_select_plan(&["anything"]),
        RowBatch::new(Schema::empty(), vec![]),
    )
    .expect("an undescribable relation is not refused");
    assert!(out.rows.is_empty());
}
