//! The one-command migration off the retired `QFS_SQL_*` / `QFS_GIT_*` connection env vars:
//! `qfs connect --import-env` renders the equivalent `CONNECT` statements for paste into a shell (or
//! straight into `qfs connect`).
//!
//! ## Why this is all that remains
//! The `connections.qfs` loader and the `QFS_SQL_*` / `QFS_GIT_*` env-var fallback are **retired**
//! (the "declared drivers are the normal way to add a service" mission): the SINGLE source of a
//! `/sql/<conn>` or `/git/<repo>` mount is the `path_binding` registry a `CONNECT` statement (or
//! `qfs connect`) persists. No env var and no config file is a working bind path any longer —
//! experimental, no backward compat, a hard break. This emitter is the ONLY place that still
//! *reads* the old `QFS_*` shape, and only to print the migration; it never binds anything.

/// Build the `CONNECT` statements equivalent to the current `QFS_SQL_*` / `QFS_GIT_*` env vars (the
/// `qfs connect --import-env` migration). Reads only the non-secret locators; a SQLite/git
/// connection carries no secret, so the output is paste-ready and secret-free. `QFS_SQL_<CONN>`
/// becomes `CONNECT /sql/<conn> TO sqlite AT '<path>'`; `QFS_GIT_<REPO>` becomes
/// `CONNECT /git/<repo> TO git AT '<path>'`.
#[must_use]
pub fn import_env_declarations() -> String {
    let mut lines: Vec<String> = Vec::new();
    for (key, value) in std::env::vars() {
        let mapped = key
            .strip_prefix("QFS_SQL_")
            .map(|name| (name, "sql", "sqlite"))
            .or_else(|| {
                key.strip_prefix("QFS_GIT_")
                    .map(|name| (name, "git", "git"))
            });
        let Some((name, family, driver)) = mapped else {
            continue;
        };
        if name.is_empty() || value.is_empty() {
            continue;
        }
        // Escape a single quote in the locator so the rendered AT '…' stays well-formed.
        let locator = value.replace('\'', "\\'");
        lines.push(format!(
            "CONNECT /{family}/{} TO {driver} AT '{locator}';",
            name.to_ascii_lowercase(),
        ));
    }
    lines.sort();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_env_renders_connect_statements_for_sql_and_git_vars() {
        // `set_var`/`remove_var` mutate process-global env — serialize on the crate-wide lock so a
        // concurrent env test never observes a half-set state (glibc `getenv` racing `setenv`).
        let _g = crate::testenv::env_guard();
        std::env::set_var("QFS_SQL_SHOP", "/data/shop.db");
        std::env::set_var("QFS_GIT_APP", "/srv/app.git");
        let out = import_env_declarations();
        assert!(out.contains("CONNECT /sql/shop TO sqlite AT '/data/shop.db';"));
        assert!(out.contains("CONNECT /git/app TO git AT '/srv/app.git';"));
        std::env::remove_var("QFS_SQL_SHOP");
        std::env::remove_var("QFS_GIT_APP");
    }
}
