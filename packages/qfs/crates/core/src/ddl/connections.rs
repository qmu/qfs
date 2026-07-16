//! Parse `CREATE CONNECTION` declarations from a `connections.qfs` source into owned
//! [`DeclaredConnection`] records — the in-language replacement for the `QFS_SQL_*` / `QFS_GIT_*`
//! env-var alias convention (the connection epic `20260630004100`).
//!
//! This lives in `qfs-core` (which already owns the `qfs-parser` edge) so the *binary* — pinned by
//! the dep-direction guard to a thin entrypoint that never touches the parser spine directly — can
//! load a connections file through the core hub. The file/env I/O stays in the binary; only the
//! pure parse lives here. Best-effort + secret-free: a statement that doesn't parse, isn't a
//! `CREATE CONNECTION`, or omits its driver is skipped. The `SECRET` clause carries only a
//! *reference* (`env:`/`vault:`), resolved lazily at use time; no secret value is read here.

use qfs_parser::{parse_statement, DdlKind, Statement};

/// One declared connection: the name (the `<conn>` path segment), the driver kind that decides the
/// path family, the optional non-secret locator, and the optional secret reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredConnection {
    /// The connection name — the `<conn>` segment in `/sql/<conn>/…`, `/git/<conn>/…`, etc.
    pub name: String,
    /// The driver kind (`sqlite`/`postgres`/`mysql`/`git`/`gmail`/…).
    pub driver: String,
    /// The `AT '<locator>'` non-secret location (a file path / URL / bucket); `None` when implicit.
    pub at_locator: Option<String>,
    /// The `SECRET '<ref>'` secret reference (`env:<VAR>` / `vault:<path>`); `None` when none.
    pub secret_ref: Option<String>,
}

/// Parse a `connections.qfs` source into the declared connections it contains. Best-effort: a
/// statement that doesn't parse, isn't a `CREATE CONNECTION`, or omits its driver is skipped.
#[must_use]
pub fn parse_connections(source: &str) -> Vec<DeclaredConnection> {
    let mut out = Vec::new();
    // Best-effort, as documented: a document that does not tokenize declares nothing.
    let Ok(stmts) = super::document::split_document(source) else {
        return out;
    };
    for (_line, stmt_src) in stmts {
        let trimmed = stmt_src.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(Statement::Ddl(ddl)) = parse_statement(trimmed) else {
            continue;
        };
        if ddl.kind != DdlKind::Connection {
            continue;
        }
        let Some(conn) = ddl.connection.as_ref() else {
            continue;
        };
        let Some(driver) = conn.driver.clone() else {
            continue;
        };
        out.push(DeclaredConnection {
            name: ddl.name.clone(),
            driver,
            at_locator: conn.at_locator.clone(),
            secret_ref: conn.secret_ref.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_declarations() {
        let src = "CREATE CONNECTION orders DRIVER sqlite AT '/data/orders.db';\n\
                   CREATE CONNECTION work DRIVER gmail SECRET 'vault:gmail/work';";
        let conns = parse_connections(src);
        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].name, "orders");
        assert_eq!(conns[0].driver, "sqlite");
        assert_eq!(conns[0].at_locator.as_deref(), Some("/data/orders.db"));
        assert!(conns[0].secret_ref.is_none());
        assert_eq!(conns[1].driver, "gmail");
        assert_eq!(conns[1].secret_ref.as_deref(), Some("vault:gmail/work"));
        assert!(conns[1].at_locator.is_none());
    }

    #[test]
    fn a_semicolon_inside_a_quoted_locator_does_not_split() {
        let conns = parse_connections("CREATE CONNECTION weird DRIVER sqlite AT '/data/a;b.db';");
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].at_locator.as_deref(), Some("/data/a;b.db"));
    }

    #[test]
    fn non_connection_statements_are_ignored() {
        let src = "CREATE POLICY p ALLOW SELECT;\n\
                   CREATE CONNECTION orders DRIVER sqlite AT '/data/orders.db';";
        let conns = parse_connections(src);
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].name, "orders");
    }

    #[test]
    fn line_comments_are_stripped_but_a_double_dash_in_a_locator_is_literal() {
        let src = "-- the shop database\n\
                   CREATE CONNECTION shop DRIVER sqlite AT '/data/shop.db'; -- trailing note\n\
                   -- a full-line comment between statements\n\
                   CREATE CONNECTION work DRIVER gmail SECRET 'vault:gmail/work';";
        let conns = parse_connections(src);
        assert_eq!(
            conns.len(),
            2,
            "comments drop out; both declarations survive"
        );
        assert_eq!(conns[0].name, "shop");
        assert_eq!(conns[0].at_locator.as_deref(), Some("/data/shop.db"));
        assert_eq!(conns[1].name, "work");
        assert_eq!(conns[1].secret_ref.as_deref(), Some("vault:gmail/work"));

        // A `--` INSIDE a quoted locator is literal, never a comment.
        let quoted = parse_connections("CREATE CONNECTION q DRIVER sqlite AT '/data/a--b.db';");
        assert_eq!(quoted.len(), 1);
        assert_eq!(quoted[0].at_locator.as_deref(), Some("/data/a--b.db"));
    }
}
