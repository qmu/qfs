// The public barrel. Re-exports the domain only — never `vendors/` (the
// anti-corruption boundary) and never `entrypoints/` (thin shells that call
// inward, so nothing imports them).
export {
  type DocumentPath,
  type DocumentSlug,
  type HeadingAnchor,
  type Route,
  isDocumentPath,
  asDocumentPath,
  documentPathString,
  isDocumentSlug,
  asDocumentSlug,
  documentSlugString,
  isHeadingAnchor,
  asHeadingAnchor,
  headingAnchorString,
  isRoute,
  asRoute,
  routeString,
} from "#qfs-viewer/domain/model/Vocabulary";

export {
  type Document,
  type ScanError,
  document,
  scanError,
} from "#qfs-viewer/domain/model/Document";

export {
  type Index,
  buildIndex,
  getDocument,
  listDocuments,
  documentCount,
  indexErrors,
  documentSource,
  withDocument,
  withoutDocument,
} from "#qfs-viewer/domain/model/Index";

export {
  type FileSystem,
  DEFAULT_ROOTS,
  PRUNED_DIRECTORIES,
  isPruned,
  isDocumentFile,
} from "#qfs-viewer/domain/model/Scan";

export {
  scan,
  walkRoot,
  readDocument,
} from "#qfs-viewer/domain/usecase/scan";

export {
  type FileChange,
  type IndexRef,
  type Timer,
  applyChange,
  indexRef,
  debouncedReload,
} from "#qfs-viewer/domain/usecase/reload";
