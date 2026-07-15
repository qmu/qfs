//! End-to-end lowering tests: parse real qfs DSL text, lower the AST into a
//! [`LogicalPlan`], then partition it. This proves the t07 carry-over O-t07-3 concretely
//! — the `WHERE` predicate is **sourced from the AST** (not dropped, as `PlanSource`
//! does) and survives into a pushed `PushedQuery` / a local residual.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qfs_driver::PushdownProfile;
use qfs_parser::{parse_statement, Statement};
use qfs_pushdown::{
    explain, lower_predicate, lower_query, partition_by_source, LogicalPlan, SourceId,
    SourceRegistry,
};
use qfs_types::{Column, ColumnType, Predicate, Schema};

fn rel_schema() -> Schema {
    Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new("name", ColumnType::Text, true),
        Column::new("age", ColumnType::Int, true),
    ])
}

/// The source resolver: the first path segment is the driver/source id.
fn source_of(segs: &[String]) -> SourceId {
    SourceId::new(segs.first().cloned().unwrap_or_default())
}

fn schema_of(_src: &SourceId) -> Schema {
    rel_schema()
}

/// A resolver that knows no transform definitions — for the non-transform lowering tests.
fn no_transform(_: &str) -> Option<qfs_types::ResolvedTransform> {
    None
}

fn pipeline_of(src: &str) -> qfs_parser::Pipeline {
    match parse_statement(src).unwrap() {
        Statement::Query(p) => p,
        other => panic!("expected a query, got {other:?}"),
    }
}

#[test]
fn path_version_survives_lowering_into_the_addressed_scan_path() {
    // A `@version` segment (e.g. a git time-travel read `/git/app@v1.2/…`) must survive lowering
    // into the addressed scan path the read driver navigates — not be dropped to the latest
    // revision. Source routing / schema lookup key on segment names only; the addressed path keeps
    // the ref. Regression: lower_source previously rendered the path from names alone.
    let pipe = pipeline_of("/git/app@v1.2/commits");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();
    let LogicalPlan::Scan { path, source, .. } = &plan else {
        panic!("expected a bare Scan, got {plan:?}");
    };
    assert_eq!(path.as_str(), "/git/app@v1.2/commits");
    // Routing still keys on the name only (the `@v1.2` is addressing, not part of the source id).
    assert_eq!(source, &SourceId::new("git"));
}

#[test]
fn where_predicate_is_sourced_from_the_ast_and_pushed() {
    // The AST WHERE survives lowering into a typed Predicate and is pushed (O-t07-3).
    let pipe = pipeline_of("/db/users |> WHERE age > 30 |> SELECT id, name");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();

    // The lowered Filter carries a real predicate (not a dropped AST).
    let LogicalPlan::Project { input, .. } = &plan else {
        panic!("expected a Project at the root, got {plan:?}");
    };
    let LogicalPlan::Filter { predicate, .. } = input.as_ref() else {
        panic!("expected a Filter under the Project");
    };
    assert!(matches!(predicate, Predicate::Cmp(_, _, _)));

    let reg = SourceRegistry::new().with(SourceId::new("db"), PushdownProfile::Full);
    let phys = partition_by_source(&plan, &reg).unwrap();
    assert_eq!(
        explain(&phys),
        "Scan[db] pushed=[where, project(id,name)]\n"
    );
}

#[test]
fn an_aliased_projection_lowers_to_a_renaming_project_expr_not_a_name_only_project() {
    // Regression: a rename (`col AS a`) must lower to the local `ProjectExpr` that maps
    // `source → alias` per row. The pushable name-only `Project` selects by source name and cannot
    // rename — routing `id AS key` there selected a non-existent column `key` and silently dropped
    // it (an aliased cross-service `SELECT filename AS name …` produced an empty relation).
    let pipe = pipeline_of("/db/users |> SELECT id AS key, name");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();
    let LogicalPlan::ProjectExpr { projections, .. } = &plan else {
        panic!("an aliased projection must lower to ProjectExpr, got {plan:?}");
    };
    // The output names are the alias and the pass-through column, in order.
    let names: Vec<&str> = projections.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["key", "name"]);
}

#[test]
fn an_all_pass_through_projection_stays_a_pushable_name_only_project() {
    // No rename, no computed term → the pushable name-only `Project` (unchanged behaviour).
    let pipe = pipeline_of("/db/users |> SELECT id, name");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();
    assert!(
        matches!(plan, LogicalPlan::Project { .. }),
        "a pass-through projection stays a name-only Project, got {plan:?}"
    );
}

#[test]
fn of_assertion_lowers_transparently_as_a_no_op() {
    // §5.6: an `of` assertion is schema-identity and carries no runtime effect — in the pushdown
    // lowering it is a pure pass-through (its structural check lives in the evaluator's fold, which
    // alone has the addressed-path schema). A following WHERE still lowers to a Filter over the Scan.
    let pipe = pipeline_of("/db/users |> of (id int, name text, age int) |> WHERE age > 1");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();
    assert!(
        matches!(plan, LogicalPlan::Filter { .. }),
        "of is transparent; the WHERE after it lowers to a Filter, got {plan:?}"
    );
}

#[test]
fn lower_predicate_maps_and_or_not_and_comparisons() {
    let pipe = pipeline_of("/db/t |> WHERE age >= 18 AND name == 'a'");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();
    let LogicalPlan::Filter { predicate, .. } = &plan else {
        panic!("expected a Filter");
    };
    assert!(matches!(predicate, Predicate::And(_, _)));
}

#[test]
fn cross_source_join_lowered_from_dsl_federates() {
    // A JOIN whose two sides resolve to different sources federates locally.
    let pipe = pipeline_of("/pg/orders |> JOIN /git/commits ON id == id");
    let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap();
    let reg = SourceRegistry::new()
        .with(SourceId::new("pg"), PushdownProfile::Full)
        .with(SourceId::new("git"), PushdownProfile::Full);
    let phys = partition_by_source(&plan, &reg).unwrap();
    assert_eq!(phys.scan_count(), 2);
    assert!(explain(&phys).starts_with("Combine[HashJoin(id = id)]"));
}

/// A resolver over one `classify` definition: `INPUT (name text)` (⇒ row-wise; `name` is carried
/// by the test relation), `OUTPUT (label text, score float)`. The forced-local + schema-fold
/// tests plan against it.
fn classify_resolver(name: &str) -> Option<qfs_types::ResolvedTransform> {
    use qfs_types::{Column, ColumnType, ResolvedTransform, Schema};
    (name == "classify").then(|| {
        ResolvedTransform::new(
            Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
            Schema::new(vec![
                Column::new("label", ColumnType::Text, true),
                Column::new("score", ColumnType::Float, true),
            ]),
        )
        .unwrap()
    })
}

#[test]
fn transform_lowers_to_a_forced_local_node_with_the_output_schema() {
    // Acceptance: `/src |> transform <def>` lowers to LogicalPlan::Transform carrying the OUTPUT
    // schema + Provenance, and the partitioner keeps it LOCAL even over a Full-pushdown source.
    let pipe = pipeline_of("/db/mail |> transform classify");
    let plan = lower_query(&pipe, &source_of, &schema_of, &classify_resolver).unwrap();
    let LogicalPlan::Transform {
        name,
        output_schema,
        mode,
        ..
    } = &plan
    else {
        panic!("expected a Transform node, got {plan:?}");
    };
    assert_eq!(name, "classify");
    assert_eq!(*mode, qfs_types::TransformMode::RowWise);
    assert!(output_schema.column("label").is_some() && output_schema.column("score").is_some());
    // Provenance tags the converted columns to the transform (not the source).
    assert_eq!(
        output_schema
            .column("label")
            .unwrap()
            .provenance
            .driver
            .as_ref()
            .unwrap()
            .as_str(),
        "transform:classify"
    );
    // Forced local: the upstream read still pushes to its source (one scan), the model call is a
    // local combine on top — even though `db` is a Full-pushdown source that could take the lot.
    let reg = SourceRegistry::new().with(SourceId::new("db"), PushdownProfile::Full);
    let phys = partition_by_source(&plan, &reg).unwrap();
    assert_eq!(phys.scan_count(), 1);
    let ex = explain(&phys);
    assert!(ex.contains("Transform(classify, row-wise)"), "{ex}");
}

#[test]
fn everything_after_a_transform_stays_local() {
    // The forced-local tail: a `WHERE` after the transform can never be pushed below it (the model
    // call is local), so it is a local residual even against a Full-pushdown source.
    let pipe = pipeline_of("/db/mail |> transform classify |> WHERE score > 0");
    let plan = lower_query(&pipe, &source_of, &schema_of, &classify_resolver).unwrap();
    let reg = SourceRegistry::new().with(SourceId::new("db"), PushdownProfile::Full);
    let phys = partition_by_source(&plan, &reg).unwrap();
    let ex = explain(&phys);
    // The scan pushes NOTHING from above the transform; the WHERE is a local Filter combine.
    assert!(
        ex.contains("Filter") && ex.contains("Transform(classify"),
        "{ex}"
    );
    assert!(
        !ex.contains("pushed=[where"),
        "the post-transform WHERE must not push: {ex}"
    );
}

#[test]
fn an_unresolved_transform_is_a_structured_lower_error() {
    let pipe = pipeline_of("/db/mail |> transform missing");
    let err = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap_err();
    assert_eq!(err.code(), "transform_not_executable");
}

/// A resolver whose definition declares `INPUT (body text)` — a column the test relation
/// (`id`, `name`, `age`) does not carry.
fn body_input_resolver(name: &str) -> Option<qfs_types::ResolvedTransform> {
    use qfs_types::{Column, ColumnType, ResolvedTransform, Schema};
    (name == "classify").then(|| {
        ResolvedTransform::new(
            Schema::new(vec![Column::new("body", ColumnType::Text, true)]),
            Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
        )
        .unwrap()
    })
}

#[test]
fn a_transform_missing_a_declared_input_column_is_a_structured_lower_error() {
    // The same by-name INPUT presence check as the eval schema fold (`transform_input_missing`):
    // both planning surfaces diagnose the missing declared input identically, at lowering, before
    // any execution path could mask the real cause.
    let pipe = pipeline_of("/db/mail |> transform classify");
    let err = lower_query(&pipe, &source_of, &schema_of, &body_input_resolver).unwrap_err();
    assert_eq!(err.code(), "transform_input_missing");
    assert!(err.to_string().contains("body"), "{err}");
}

#[test]
fn a_transform_input_check_sees_through_shaping_and_projection_stages() {
    // The check runs on the RUNNING relation, not the scan schema: a projection that drops the
    // declared input column is caught; one that keeps it passes.
    let dropped = pipeline_of("/db/mail |> SELECT id, age |> transform classify");
    let err = lower_query(&dropped, &source_of, &schema_of, &classify_resolver).unwrap_err();
    assert_eq!(err.code(), "transform_input_missing");

    let kept = pipeline_of("/db/mail |> WHERE age > 1 |> SELECT id, name |> transform classify");
    assert!(lower_query(&kept, &source_of, &schema_of, &classify_resolver).is_ok());
}

/// A two-stage chain resolver: `extract` (name text → summary text) feeds `summarize`
/// (summary text → digest text). Stage b's INPUT is satisfied ONLY by stage a's OUTPUT.
fn chain_resolver(name: &str) -> Option<qfs_types::ResolvedTransform> {
    use qfs_types::{Column, ColumnType, ResolvedTransform, Schema};
    match name {
        "extract" => Some(
            ResolvedTransform::new(
                Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("summary", ColumnType::Text, true)]),
            )
            .unwrap(),
        ),
        "summarize" => Some(
            ResolvedTransform::new(
                Schema::new(vec![Column::new("summary", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("digest", ColumnType::Text, true)]),
            )
            .unwrap(),
        ),
        _ => None,
    }
}

#[test]
fn a_two_stage_transform_chain_lowers_with_the_second_stage_typed_by_the_first_output() {
    // `… |> transform extract |> transform summarize`: stage b (summarize) is typed by stage a's
    // OUTPUT — its INPUT column `summary` is produced by `extract`, not read from the source. The
    // plan is nested Transform(summarize ⟵ Transform(extract ⟵ scan)), and the whole tail stays
    // local (two model calls, both local combines).
    let pipe = pipeline_of("/db/mail |> transform extract |> transform summarize");
    let plan = lower_query(&pipe, &source_of, &schema_of, &chain_resolver).unwrap();
    let LogicalPlan::Transform {
        name,
        output_schema,
        input,
        ..
    } = &plan
    else {
        panic!("expected the outer Transform (summarize), got {plan:?}");
    };
    assert_eq!(name, "summarize", "the terminal stage is the outer node");
    assert!(
        output_schema.column("digest").is_some(),
        "the chain's relation is the LAST stage's OUTPUT"
    );
    // The inner node is stage a (extract), producing the `summary` the outer stage consumes.
    let LogicalPlan::Transform { name: inner, .. } = input.as_ref() else {
        panic!("expected the inner Transform (extract), got {input:?}");
    };
    assert_eq!(inner, "extract");
    // Forced-local stacking: two transforms over a Full-pushdown source still push only the scan.
    let reg = SourceRegistry::new().with(SourceId::new("db"), PushdownProfile::Full);
    let phys = partition_by_source(&plan, &reg).unwrap();
    assert_eq!(phys.scan_count(), 1, "both model stages are local");
    let ex = explain(&phys);
    assert!(
        ex.contains("Transform(extract") && ex.contains("Transform(summarize"),
        "{ex}"
    );
}

#[test]
fn an_incompatible_transform_chain_fails_at_lowering_naming_the_unsatisfied_column() {
    // Stage b's declared INPUT is NOT produced by stage a's OUTPUT → the chain fails at PLAN time
    // (PREVIEW), before any model call, naming the missing column. `summarize` needs `summary`, but
    // `extract` here OUTPUTs only `title` — the schema handoff is checked, not assumed.
    fn mismatched(name: &str) -> Option<qfs_types::ResolvedTransform> {
        use qfs_types::{Column, ColumnType, ResolvedTransform, Schema};
        match name {
            "extract" => Some(
                ResolvedTransform::new(
                    Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
                    Schema::new(vec![Column::new("title", ColumnType::Text, true)]),
                )
                .unwrap(),
            ),
            "summarize" => chain_resolver("summarize"),
            _ => None,
        }
    }
    let pipe = pipeline_of("/db/mail |> transform extract |> transform summarize");
    let err = lower_query(&pipe, &source_of, &schema_of, &mismatched).unwrap_err();
    assert_eq!(err.code(), "transform_input_missing");
    assert!(
        err.to_string().contains("summary"),
        "the error names the unsatisfied INPUT column: {err}"
    );
}

#[test]
fn lower_predicate_rejects_non_predicate_expression() {
    // A bare column is not a comparison predicate — a structured error, never silently
    // dropped (so the planner never loses a filter).
    let pipe = pipeline_of("/db/t |> WHERE id");
    let err = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap_err();
    assert_eq!(err.code(), "unsupported_predicate");
}

#[test]
fn lower_predicate_rejects_arithmetic_until_pushdown_ir_supports_it() {
    let pipe = pipeline_of("/db/t |> WHERE age + 1 > 30");
    let err = lower_query(&pipe, &source_of, &schema_of, &no_transform).unwrap_err();
    assert_eq!(err.code(), "unsupported_predicate");
    assert!(
        err.to_string().contains("arithmetic"),
        "error should name the unsupported expression shape: {err}"
    );
}

#[test]
fn lower_predicate_handles_like_and_in_and_between() {
    use qfs_parser::Expr;
    // Drive lower_predicate directly over parsed expressions through the pipeline.
    for (src, ok) in [
        ("/db/t |> WHERE name LIKE 'a%'", true),
        ("/db/t |> WHERE age IN (1, 2, 3)", true),
        ("/db/t |> WHERE age BETWEEN 1 AND 9", true),
    ] {
        let pipe = pipeline_of(src);
        let plan = lower_query(&pipe, &source_of, &schema_of, &no_transform);
        assert_eq!(plan.is_ok(), ok, "{src}");
    }
    // And lower_predicate is exposed for direct use over an AST Expr.
    let pipe = pipeline_of("/db/t |> WHERE age == 5");
    if let qfs_parser::PipeOp::Where(e) = &pipe.ops[0] {
        let p = lower_predicate(e).unwrap();
        assert!(matches!(p, Predicate::Cmp(_, _, _)));
    } else {
        let _: &Expr; // unreachable; keep the import meaningful
        panic!("expected a WHERE op");
    }
}
