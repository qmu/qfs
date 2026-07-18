import { type SoftStr } from "plgg";
import {
  type Row,
  type Field,
  fieldText,
} from "plggmatic/Declare/model/Row";

/**
 * One declared choice filter of a {@link Query} — a typed
 * query field (mission decision, 2026-07-12, point 4):
 * `id` names the URL parameter and the input Msg, `label`
 * captions the control, `field` names the row field the
 * closed equality test reads, and `options` is the
 * declared, closed value set (WebMCP tool schemas derive
 * their enumerations from it).
 */
export type QueryChoice = Readonly<{
  id: SoftStr;
  label: SoftStr;
  field: SoftStr;
  options: ReadonlyArray<SoftStr>;
}>;

/** Constructs a {@link QueryChoice}. */
export const queryChoice = (
  id: SoftStr,
  label: SoftStr,
  field: SoftStr,
  options: ReadonlyArray<SoftStr>,
): QueryChoice => ({ id, label, field, options });

/**
 * A declarative filter over a {@link Collection}'s rows:
 * a keyword (case-insensitive substring over `Row.label`)
 * plus the declared {@link QueryChoice} filters. The live
 * values live in the scheduled model and are reflected
 * into the URL, so a filtered arrangement is a shareable
 * address. Client-side evaluation is CLOSED to this pair
 * — substring for the keyword, equality for a choice;
 * predicate expressions belong to the manifest/DSL and
 * execute at the source.
 */
export type Query = Readonly<{
  placeholder: SoftStr;
  choices: ReadonlyArray<QueryChoice>;
}>;

/**
 * Constructs a {@link Query}. The historical
 * one-argument form is the degenerate keyword-only
 * declaration.
 */
export const query = (
  placeholder: SoftStr,
  choices: ReadonlyArray<QueryChoice> = [],
): Query => ({ placeholder, choices });

/**
 * Whether a row label matches a query text — the shared
 * filter semantics. Empty text matches everything (an
 * empty filter is not a filter). Case-insensitive
 * substring, so it is total and never throws.
 */
export const matchesQuery = (
  text: SoftStr,
  label: SoftStr,
): boolean =>
  text === "" ||
  label
    .toLowerCase()
    .includes(text.toLowerCase());

/**
 * Whether a row satisfies one chosen choice value — the
 * closed equality half of the query semantics: the row
 * field labelled `field` must display exactly the chosen
 * value. An empty choice matches everything; a row
 * without the field matches nothing (total either way).
 */
export const matchesChoice = (
  chosen: SoftStr,
  field: SoftStr,
  row: Row,
): boolean =>
  chosen === "" ||
  row.fields.some(
    (f: Field) =>
      f.label === field &&
      fieldText(f.value) === chosen,
  );
