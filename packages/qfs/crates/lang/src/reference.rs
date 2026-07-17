//! `qfs-lang::reference` — the language **reference surface** (ticket t40).
//!
//! This module exposes the two pieces `cargo xtask gen-docs` renders `docs/language.md`
//! from, so the published reference is **derived from the binary's own data**, never
//! hand-authored twice (anti-drift, blueprint §3 governance):
//!
//! - [`RESERVED_KEYWORDS`] — the §3 frozen reserved-word set. It is a **re-export alias**
//!   of [`crate::keywords::KEYWORDS`], the single committed fixture the freeze test locks.
//!   There is deliberately **no second transcription**: adding a keyword to `KEYWORDS`
//!   (the one source) flows straight into the generated docs, and the t40 docs-drift golden
//!   makes the doc fail CI until it is regenerated. The frozen-keyword test in this module
//!   asserts the alias and the source are the very same slice.
//! - [`language_model_reference`] — the two-layer language model prose and the stage/combinator
//!   equivalence table rendered above the grammar.
//! - [`grammar_ebnf`] — the pipe-SQL grammar in EBNF, the legible contract the AI agent
//!   reads. It is a `&'static str`; the `gen-docs` renderer embeds it verbatim into
//!   `docs/language.md` between fenced markers, so the committed grammar cannot drift from
//!   this constant.
//!
//! Both are pure data (no I/O, no deps) — this crate must stay wasm-clean (boundary guard
//! B7), so the reference surface adds nothing impure.

use crate::keywords::KEYWORDS;

/// The frozen reserved-keyword set (blueprint §3), as the documentation surface reads it.
///
/// This is an **alias of [`crate::keywords::KEYWORDS`]** — the *single* committed fixture —
/// not a second hand-transcription. `gen-docs` renders the reserved-word table in
/// `docs/language.md` from this slice; because it *is* `KEYWORDS`, adding/removing a keyword
/// in the one source changes the generated docs, and the docs-drift golden fails CI until
/// `docs/language.md` is regenerated. The frozen-keyword test
/// ([`tests::reserved_keywords_is_the_frozen_set`]) locks this identity.
pub const RESERVED_KEYWORDS: &[&str] = KEYWORDS;

/// The generated language reference's prose model for the two-layer language.
///
/// This lives in `qfs-lang`, beside the frozen vocabulary and EBNF, so the generated
/// `docs/language.md` carries its language-self-description from the language crate rather than
/// from hand-authored markdown. The equivalence table is a semantic reading, not a promise that
/// `filter(rel, ...)` or `project(rel, ...)` are exported relation-valued stdlib calls today.
#[must_use]
pub const fn language_model_reference() -> &'static str {
    "\
qfs is a **closed relational pipe algebra** over typed paths, plus a **total, pure, row-scoped
expression language** where functions are values. A query source produces a relation; every
`|>` stage has a visible typing rule from one relation shape to the next, so the planner can
type-check it, push it down when safe, and keep the preview/commit gate honest.

The expression layer is where scalar functions, lambdas, comparisons, predicates, and arithmetic
live. Expressions may compute values for the current row; they do not perform I/O and do not hide
new stages from the planner. Effect seams stay explicit as write stages, `call driver.action(...)`,
or the declared `transform` stage, all of which still pass through preview before commit.

## Stage readings

Pipe stages read as notation for relation combinators over the inflowing relation. These are
equivalences for understanding the closed algebra, not extra exported syntax:

| stage form | combinator reading |
| --- | --- |
| `where p` | `filter(rel, (row) => p)` |
| `select a, b` | `project(rel, [a, b])` |
| `extend n = e` | `map(rel, (row) => row + { n: e })` |
| `set n = e` | `map(rel, (row) => row with n = e)` |
| `aggregate ... group by ...` | `group_reduce(rel, keys, aggregates)` |
| `order by k`, `limit n`, `distinct` | `sort` / `take` / `dedupe` over the same schema |
| `join r on p` | `join(rel, r, (left, right) => p)` |
| `union r`, `except r`, `intersect r` | set operations over compatible relation types |
| `expand field` | `flat_map` over a nested collection field |
| `decode fmt`, `encode fmt` | codec-declared relation/blob transforms |
| `transform name` | declared model-call stage with explicit input/output relation types |
| `switch col { 'a' => arm, else => arm }` | `partition(rel, col)` routed to declared effect arms — the arm union is the declared effect set |
| `follow field` | declared-driver second fetch: the delivered row's URL field GETs its raw bytes as a one-row `content` relation (declared view bodies only) |
| write stages and `call driver.action(...)` | effect seams that construct a plan, then pass through preview/commit |
"
}

/// The pipe-SQL grammar in EBNF (blueprint §2/§3) — the stable contract surface an AI agent
/// (and a human operator) reads. Returned as a `&'static str` so `gen-docs` can embed it
/// verbatim into `docs/language.md`; the docs-drift golden then locks the committed grammar
/// to this constant.
///
/// The grammar is intentionally the **shape** of the closed core (the frozen keywords +
/// operators wired into statement/pipeline forms), not a parser-complete grammar — E1 owns
/// the executable grammar. Every terminal here is drawn from the frozen
/// [`RESERVED_KEYWORDS`] / [`crate::keywords::OPERATORS`] sets, so the reference and the
/// lexer cannot disagree about the vocabulary.
#[must_use]
pub const fn grammar_ebnf() -> &'static str {
    // Kept as one frozen literal. Quoted lowercase terminals are reserved keywords (see
    // RESERVED_KEYWORDS, t74 decision S); quoted UPPERCASE terminals are word operators
    // (the OPERATORS set: AND/OR/NOT/LIKE/IN/ANY/BETWEEN plus arithmetic). Unquoted lowercase names are
    // nonterminals. A new backend adds ZERO terminals here.
    "\
(* qfs pipe-SQL grammar (EBNF) — blueprint §2/§3.                                  *)
(* The closed core: every quoted lowercase terminal is a frozen reserved keyword   *)
(* (see RESERVED_KEYWORDS, t74 decision S); a new backend adds ZERO terminals here. *)

(* A program is zero or more let bindings (M6 functional core, ticket t60) in       *)
(* scope for the statements that follow them — one statement per line, no `;`.       *)
program       = { binding } , statement ;
(* A let binds a relation (a pipeline) OR a first-class VALUE — a lambda or a scalar     *)
(* (M6, ticket t61, decision H \"functions are values\"). A named function is just a        *)
(* let-bound lambda — there is NO `def` keyword.                                          *)
binding       = \"let\" , name , \"=\" , ( pipeline | lambda | literal ) ;

(* A statement is a pipeline (which may terminate in a write stage, decision Q), the      *)
(* source-less verb-leading literal write, or a transaction block (M6, ticket t62); an     *)
(* optional plan_op wraps any of them.                                                    *)
statement     = ( pipeline | effect_literal | transaction ) , [ plan_op ] ;

(* ---- transactions (M6, ticket t62, decision G) ---- *)
(* A reversible-only, all-or-nothing block: a `;`-separated sequence of effect            *)
(* statements committed in source order. Every effect inside MUST be reversible —          *)
(* an irreversible effect (remove, an irreversible call) is a hard eval-time error, never  *)
(* a needs-an-ack prompt — so the block can always roll back. No nesting, no let inside.    *)
transaction   = \"transaction\" , \"{\" , [ effect_stmt , { \";\" , effect_stmt } , [ \";\" ] ] , \"}\" ;
effect_stmt   = effect_literal | ( pipeline , effect_stage ) ;

(* A pipeline is a source threaded through |> stages. Decision R (t73): the source     *)
(* position needs no `from` — a leading `/path` (or a let-bound name) IS the source.   *)
pipeline      = source , { \"|>\" , stage } ;

source        = path | id_ref | name ;   (* name = a let-bound relation *)
path          = \"/\" , segment , { \"/\" , segment } ;   (* absolute only, no cwd *)
segment       = bare_segment | quoted_segment ;
quoted_segment = \"'\" , { any_char_except_quote_or_slash | \"''\" } , \"'\" ;
id_ref        = \"id:\" , token ;

(* A QUOTED segment carries any character literally — spaces, `?`, `#`, `&`, `(`, Unicode —  *)
(* so a real file name is addressable as one path: /drive/my/'Q3 budget (final)?.xlsx'.      *)
(* Escape a quote by doubling it (''). A quoted `?`/`*` is a LITERAL character, never a      *)
(* glob: /drive/my/'report?.pdf' is one file, /drive/my/report?.pdf still globs. A `/` may   *)
(* not appear inside quotes (the separator is structural). Bare segments are unchanged.      *)

stage         = query_stage | effect_stage | codec_stage | call_stage ;

(* ---- query / transform (read) ---- *)
query_stage   = \"where\" , predicate
              | \"select\" , projection
              | \"extend\" , assignment
              | \"set\" , assignment
              | \"aggregate\" , agg_list
              | \"group by\" , column_list
              | \"order by\" , sort_list
              | \"limit\" , integer
              | \"distinct\"
              | \"join\" , source , \"on\" , predicate
              | \"union\" , source
              | \"except\" , source
              | \"intersect\" , source
              | \"expand\" , column
              | transform_stage
              | switch_stage
              | follow_stage ;

(* ---- transform (the model-calling stage, blueprint §15, decision W) ---- *)
(* `transform` is a CONTEXTUAL identifier, NOT a frozen keyword — the closed core stays 39.  *)
(* The stage names a `create transform` definition; how the upstream relation feeds the model *)
(* (row-wise / relation-wise / extraction) is DERIVED from the definition's input shape, not  *)
(* written here. A transform-bearing statement is effect-bearing: it previews (no model call)  *)
(* and commits (the model runs) through the plan_op gate, and its commit is irreversible.       *)
transform_stage = \"transform\" , name ;   (* contextual ident, not reserved *)

(* ---- switch (the model-routing stage, blueprint §18) ---- *)
(* `switch` and `else` are CONTEXTUAL identifiers — the closed core stays 39. The stage is    *)
(* TERMINAL-only: it partitions the incoming relation by the discriminant column's value and  *)
(* routes each partition to a declared effect arm (an `insert into`/`upsert into` write, or a *)
(* terminal effect `call`). The model's choice (a `transform` output column) selects AMONG    *)
(* pre-declared arms — every arm's effect is previewed BEFORE any model runs (the statement's *)
(* declared effect set is the arm UNION); at commit the source materializes once, an arm with *)
(* an empty partition never fires, and an unmatched or non-text value falls to `else` (which   *)
(* is mandatory and written last).                                                             *)
switch_stage  = \"switch\" , column , \"{\" , switch_arm , { \",\" , switch_arm } , \"}\" ;
switch_arm    = ( string | \"else\" ) , \"=>\" , arm_body ;

(* ---- follow (the declared-driver second-fetch stage, blueprint §13) ---- *)
(* `follow` is a CONTEXTUAL identifier — the closed core stays 39. ONLY meaningful inside a    *)
(* declared driver view body (`create view ... as /http/<self>/... |> decode json |> follow    *)
(* url_field`): the named field of the single delivered row is the URL of a second GET whose   *)
(* raw bytes become a one-row `content` relation (the declared file-download shape). The       *)
(* follow request carries NO driver credential (the URL is self-authorizing). Anywhere else    *)
(* the stage is a structured refusal. A wire path may carry a `?query=…` suffix behind its     *)
(* last segment (`/files/{file}?create_download_url=1`).                                        *)
follow_stage  = \"follow\" , name ;   (* contextual ident, not reserved *)
arm_body      = [ query_stage , { \"|>\" , query_stage } , \"|>\" ] ,
                ( \"insert into\" , target , [ \"returning\" , projection ]
                | \"upsert into\" , target , [ \"returning\" , projection ]
                | call_stage ) ;

(* ---- effects (write) — decision Q (t72): a write reads as dataflow. With inflowing  *)
(* rows it is a TERMINAL pipeline stage; the upstream relation is what it writes        *)
(* (insert/upsert) or the rows it rewrites/removes in place (update/remove, whose target *)
(* and where are the upstream `/path |> where …`). returning rides as a trailing stage.  *)
effect_stage  = \"insert into\" , target
              | \"upsert into\" , target
              | \"update\" , \"set\" , assignment
              | \"remove\"
              | \"returning\" , projection ;

(* The source-less LITERAL write leads with the verb (no inflowing rows to consume);     *)
(* `values` supplies the rows inline. This is the only write form that is not a stage.    *)
(* The optional `|> encode <fmt>` between target and `values` names the wire-body          *)
(* encoding of a declared MAP upload (blueprint §13, e.g. `|> encode multipart` for a      *)
(* multipart/form-data POST); without it the wire body is the default JSON object.          *)
effect_literal= \"insert into\" , target , [ \"|>\" , \"encode\" , format ] , \"values\" , row_list , [ \"returning\" , projection ]
              | \"upsert into\" , target , [ \"|>\" , \"encode\" , format ] , \"values\" , row_list , [ \"returning\" , projection ] ;

(* ---- procedures (the irreducible state transitions) ---- *)
call_stage    = \"call\" , qualified_proc , \"(\" , [ arg_list ] , \")\" ;
qualified_proc= driver_id , \".\" , action ;          (* e.g. mail.send, git.merge *)

(* ---- codecs (blob <-> relational) ---- *)
(* A declared MAP upload's `|> encode multipart` (blueprint §13) additionally accepts     *)
(* `multipart` — a WIRE-BODY encoding (multipart/form-data), not a relational codec.       *)
codec_stage   = \"decode\" , format | \"encode\" , format ;
format        = \"json\" | \"jsonl\" | \"yaml\" | \"toml\" | \"csv\" | \"md\" | \"multipart\" ;

(* ---- plan operator (preview is default; commit applies) ---- *)
plan_op       = \"preview\" | \"commit\" ;

(* ---- predicate / expression core ---- *)
(* Decision O (t70): `=` ALWAYS binds (let / extend / set / named arg);             *)
(* equivalence is the explicit `==`. Unlike SQL, a lone `=` never compares.         *)
predicate     = expr , { ( \"AND\" | \"OR\" ) , expr } | \"NOT\" , predicate ;
expr          = arithmetic , [ comparison , arithmetic ] ;
arithmetic    = product , { ( \"+\" | \"-\" ) , product } ;
product       = operand , { ( \"*\" | \"/\" ) , operand } ;
comparison    = \"==\" | \"<>\" | \"<\" | \">\" | \"<=\" | \">=\"
              | \"LIKE\" | \"~\" | \"IN\" | \"ANY\" | \"BETWEEN\" ;
operand       = column | literal | call | lambda ;

(* ---- lambdas as values + higher-order fns (M6, ticket t61, decision H) ---- *)
(* Functions are values: a lambda rides the EXPRESSION grammar and reuses the `=>`   *)
(* token — it adds ZERO keywords and ZERO operators (the closed core is untouched).   *)
(* The parenthesised parameter list distinguishes it from a named arg / sub-expr.     *)
lambda        = \"(\" , [ param , { \",\" , param } ] , \")\" , \"=>\" , expr ;
param         = name , [ \":\" , type ] ;   (* `: type` is parsed-and-retained *)
(* `map` / `filter` / `reduce` are ordinary stdlib functions (a `call`), NOT keywords: *)
(*   map(collection, lambda) | filter(collection, lambda) | reduce(collection, lambda, init) *)
call          = name , \"(\" , [ expr , { \",\" , expr } ] , \")\" ;

(* ---- server DDL (sugar over the write surface, blueprint §10) ---- *)
ddl           = \"create\" , ( endpoint | trigger | job | view | webhook | policy | transform_def ) ;
endpoint      = \"endpoint\" , name , \"do\" , statement ;
trigger       = \"trigger\" , name , \"on\" , event , \"do\" , statement ;
job           = \"job\" , name , \"every\" , interval , \"do\" , statement ;
view          = ( \"view\" | \"materialized view\" ) , name , \"as\" , pipeline ;
webhook       = \"webhook\" , name , \"do\" , statement ;
policy        = \"policy\" , name , predicate ;

(* `create transform` (blueprint §15) declares a model-calling definition — it desugars to an   *)
(* `insert into /transform` write (like `create table` / `connect`), and `remove transform <name>` *)
(* to a `remove /transform/<name>`. `transform`, `input`, `output`, `provider`, `model`, `effort`,  *)
(* and `secret` are all CONTEXTUAL idents (the closed core stays 39). A `secret` is a REFERENCE    *)
(* (`env:<var>` / `vault:<path>`), NEVER an inline value — resolved lazily at commit.               *)
transform_def = \"transform\" , name ,
                \"input\"  , \"(\" , column_type_list , \")\" ,
                \"output\" , \"(\" , column_type_list , \")\" ,
                \"provider\" , word_or_string , \"model\" , word_or_string ,
                [ \"effort\" , word_or_string ] , [ \"secret\" , string ] ;

(* Lowercase nonterminals (bare_segment, token, projection, assignment, agg_list,  *)
(* column_list, sort_list, integer, target, row_list, column, arg_list, action,    *)
(* driver_id, name, event, interval, literal, column_type_list, word_or_string,    *)
(* string) are E1's lexical/structural detail.                                      *)
"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keywords::{KEYWORDS, OPERATORS};

    /// Frozen-keyword test (t40 acceptance): `RESERVED_KEYWORDS` *is* the one committed
    /// frozen set `KEYWORDS` — same pointer, same length, same elements. There is no second
    /// transcription, so adding a keyword without regenerating the docs is caught by the
    /// docs-drift golden, and adding one without updating the source slice is impossible.
    #[test]
    fn reserved_keywords_is_the_frozen_set() {
        // Identity: the alias points at the exact same slice (no copy, no re-transcription).
        assert!(
            std::ptr::eq(RESERVED_KEYWORDS, KEYWORDS),
            "RESERVED_KEYWORDS must be the SAME slice as KEYWORDS, not a re-transcription"
        );
        assert_eq!(RESERVED_KEYWORDS.len(), KEYWORDS.len());
        assert_eq!(RESERVED_KEYWORDS, KEYWORDS);
        // And it carries the §3-frozen count (mirrors keywords.rs's own freeze test, so a
        // keyword smuggled in anywhere fails here too).
        assert_eq!(
            RESERVED_KEYWORDS.len(),
            39,
            "the closed-core reserved-word set is frozen at 39 entries \
             (blueprint §3 + t60 `LET` − t73 `FROM` + t62 `TRANSACTION`)"
        );
    }

    /// The grammar references only the frozen vocabulary: every reserved keyword and every
    /// operator named in the EBNF is a member of the frozen sets, and conversely every frozen
    /// keyword appears in the grammar — so the reference cannot drift from the lexer's
    /// vocabulary. (A purely textual check; structural parsing is E1's concern.)
    #[test]
    fn grammar_uses_only_frozen_vocabulary() {
        let ebnf = grammar_ebnf();
        // Every frozen keyword's quoted form appears as a terminal in the grammar.
        for kw in RESERVED_KEYWORDS {
            let quoted = format!("\"{kw}\"");
            assert!(
                ebnf.contains(&quoted),
                "frozen keyword {kw} must appear as a terminal in grammar_ebnf()"
            );
        }
        // Every operator's quoted form appears too (spot the comparison/logical/pipe set).
        for op in OPERATORS {
            let quoted = format!("\"{op}\"");
            assert!(
                ebnf.contains(&quoted),
                "frozen operator {op} must appear in grammar_ebnf()"
            );
        }
    }
}
