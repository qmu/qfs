//! blueprint §13 — **is my installed declaration still the shipped one?** (ticket 20260812141223).
//!
//! An operator installs a declared driver by previewing and committing its statements, which
//! desugar to `/sys/drivers` rows. From then on *the installed rows are the driver* — the shipped
//! file is never consulted again. So when a shipped declaration is corrected (the Chatwork
//! `force=1` fix, the Slack `CREATE LOOKUP` rewrite), the operator who already installed it keeps
//! running the old rows and has no way to find out. This module is the answer: it recomputes,
//! per installed declaration, whether it still matches the declaration **this binary ships**.
//!
//! ## Identity is DERIVED, never stamped
//! The ticket proposed recording the installed declaration's identity at install time. It is not
//! written that way, for two reasons that survived the survey (PR #59):
//!
//! - The desugar sees **one `CREATE …` statement at a time** and never the document, so a
//!   whole-script stamp cannot be written there without the side-channel write the ticket itself
//!   forbids.
//! - A stamp is a *claim the installer made*; a derivation is a *fact about what is installed*.
//!   Only the derivation answers the case the ticket flags as unhandled — a statement REMOVED from
//!   the shipped script leaves its row behind, superseded by nothing, and a stamp would still read
//!   "current" while the derived comparison reports the leftover key.
//!
//! Nothing is stored, so there is **no migration and no new row kind**, and the answer cannot go
//! stale relative to the rows it describes.
//!
//! ## The comparison is per declaration ITEM, not per script
//! Comparing raw script bytes would call a comment-only edit a change; comparing one whole-model
//! blob would say "something moved" without saying what. Both sides are reduced to the same
//! `(key, canonical value)` map — one entry per `CREATE DRIVER`/`TYPE`/`VIEW`/`MAP`/`LOOKUP` — so
//! the difference is reported as the **keys that disagree**, which are exactly the statements an
//! operator re-installs. Install ORDER is not identity: the map is ordered by key.
//!
//! The installed side is reduced through the SAME newest-row-per-`(kind, name, verb)` rule
//! [`assemble`](crate::declared_driver) applies, so a registry carrying superseded rows from the
//! append era compares as the declaration that is actually live.
//!
//! ## Zero network, zero credentials
//! Both sides are local: `/sys/drivers` rows out of the System DB, and the shipped text embedded in
//! the binary (`qfs_skill::DECLARED_DRIVERS`). No declaration is ever fetched — the ticket puts
//! that out of scope, and a currency check that needed the network could not answer offline.

use std::collections::BTreeMap;

use crate::declared_driver::DriverRow;

/// The three answers `/sys/declarations` distinguishes — a closed set, because the ticket's whole
/// point is that "not a shipped declaration" must never be reported as "stale".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// The installed declaration matches the shipped one item for item.
    Current,
    /// It names a shipped declaration and differs from it — re-installing the listed statements
    /// brings it up to date.
    Stale,
    /// No shipped declaration declares this driver name: a locally authored declaration, which has
    /// nothing to be older than.
    Local,
}

impl Status {
    /// The stable column value (`current` / `stale` / `local`).
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Local => "local",
        }
    }
}

/// One row of `/sys/declarations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationReport {
    /// The installed declaration's driver name (the `CONNECT … TO <name>` target).
    pub driver: String,
    pub status: Status,
    /// The shipped asset the comparison ran against — `None` for [`Status::Local`], which has no
    /// shipped counterpart.
    pub shipped: Option<String>,
    /// The declaration keys that disagree, in key order. Empty unless [`Status::Stale`].
    pub differs: Vec<String>,
}

/// Every installed declaration's currency, driver name order. Reads the System DB (a pure local
/// read) and the binary's own embedded assets; returns an empty list when no System DB resolves —
/// a host with no declarations installed has nothing to report, which is not an error.
pub(crate) fn declaration_reports() -> Vec<DeclarationReport> {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return Vec::new();
    };
    let conn = sys.into_db().into_connection();
    let installed = crate::declared_driver::rows_from_conn(&conn).unwrap_or_default();
    reports_from(&installed, &shipped_declarations())
}

/// The shipped side, parsed once per call: each embedded asset reduced to the rows its statements
/// desugar to. An asset that fails to parse contributes nothing rather than poisoning the report —
/// the shipped corpus is ratcheted by its own tests, so a parse failure here is a bug in this
/// binary, not a fact about the operator's installation.
fn shipped_declarations() -> Vec<(String, Vec<DriverRow>)> {
    qfs_skill::DECLARED_DRIVERS
        .iter()
        .map(|a| (a.label.to_string(), rows_from_script(a.source)))
        .collect()
}

/// The pure core: compare each installed declaration against the shipped corpus. Separated from
/// the System-DB read so the whole three-outcome contract is testable with no DB and no I/O.
pub(crate) fn reports_from(
    installed: &[DriverRow],
    shipped: &[(String, Vec<DriverRow>)],
) -> Vec<DeclarationReport> {
    let mut names: Vec<String> = installed
        .iter()
        .filter(|r| r.kind == "driver")
        .map(|r| r.name.clone())
        .collect();
    names.sort();
    names.dedup();

    names
        .into_iter()
        .map(|driver| {
            let mine = items_for(installed, &driver);
            match shipped
                .iter()
                .find(|(_, rows)| rows.iter().any(|r| r.kind == "driver" && r.name == driver))
            {
                None => DeclarationReport {
                    driver,
                    status: Status::Local,
                    shipped: None,
                    differs: Vec::new(),
                },
                Some((label, rows)) => {
                    let theirs = items_for(rows, &driver);
                    let differs = differing_keys(&mine, &theirs);
                    DeclarationReport {
                        driver,
                        status: if differs.is_empty() {
                            Status::Current
                        } else {
                            Status::Stale
                        },
                        shipped: Some(label.clone()),
                        differs,
                    }
                }
            }
        })
        .collect()
}

/// The keys present on exactly one side, or present on both with different canonical values — in
/// key order. A key the operator's registry carries and the shipped script does not is reported
/// like any other difference: that is the "statement removed upstream, row left behind" case the
/// ticket names, and it is a real reason the installation is not the shipped one.
fn differing_keys(
    mine: &BTreeMap<String, String>,
    theirs: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut keys: Vec<&String> = mine.keys().chain(theirs.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| mine.get(*k) != theirs.get(*k))
        .cloned()
        .collect()
}

/// Reduce one side's rows to the declaration items belonging to `driver`, newest row per
/// `(kind, name, verb)` — the same resolution the live registry applies, so what is compared is
/// what actually runs.
fn items_for(rows: &[DriverRow], driver: &str) -> BTreeMap<String, String> {
    let mut newest: BTreeMap<(&str, &str, &str), &DriverRow> = BTreeMap::new();
    for r in rows {
        if owning_driver(&r.kind, &r.name) != Some(driver) {
            continue;
        }
        let key = (
            r.kind.as_str(),
            r.name.as_str(),
            r.verb.as_deref().unwrap_or(""),
        );
        match newest.get(&key) {
            Some(prev) if prev.id >= r.id => {}
            _ => {
                newest.insert(key, r);
            }
        }
    }
    newest
        .into_values()
        .map(|r| (item_key(r), item_value(r)))
        .collect()
}

/// Which declared driver a row belongs to. `driver` rows name it directly; `view`/`map` rows by
/// their node path's leading segment; `lookup` rows by their key's leading segment; `type` rows by
/// the segment after `/type/`. A `table` row (a qfs table contract, not an integration) belongs to
/// no declared driver.
fn owning_driver<'a>(kind: &str, name: &'a str) -> Option<&'a str> {
    let seg = |s: &'a str, skip: usize| {
        s.trim_start_matches('/')
            .split('/')
            .nth(skip)
            .filter(|x| !x.is_empty())
    };
    match kind {
        "driver" => Some(name),
        "view" | "map" | "lookup" => seg(name, 0),
        "type" => seg(name, 1),
        _ => None,
    }
}

/// The item's identity within its declaration — one statement's address, stable across installs.
fn item_key(r: &DriverRow) -> String {
    match r.kind.as_str() {
        "driver" => "driver".to_string(),
        "map" => format!("map {} {}", r.verb.as_deref().unwrap_or(""), r.name),
        kind => format!("{kind} {}", r.name),
    }
}

/// The item's canonical value — every column that carries declared MEANING, and nothing else. The
/// row id and the install timestamp are deliberately absent: re-installing an unchanged statement
/// writes a new row with a new id, and that is not a change to the declaration.
fn item_value(r: &DriverRow) -> String {
    let f = |v: &Option<String>| v.clone().unwrap_or_default();
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        f(&r.base_url),
        f(&r.auth),
        f(&r.pagination),
        f(&r.of_type),
        f(&r.body),
        r.irreversible,
        f(&r.pushdown),
    )
}

/// Reduce a declaration program to the `/sys/drivers` rows its statements desugar to — the shipped
/// side of the comparison, built through the **production** splitter and grammar rather than a
/// second reader of the same bytes. A statement that is not a `/sys/drivers` insert (or that does
/// not parse) contributes nothing.
pub(crate) fn rows_from_script(script: &str) -> Vec<DriverRow> {
    let Ok(stmts) = qfs_core::ddl::document::split_document(script) else {
        return Vec::new();
    };
    stmts
        .into_iter()
        .enumerate()
        .filter_map(|(i, (_line, src))| {
            let stmt = qfs_exec::parse(&src).ok()?;
            crate::declared_driver::driver_row_from_statement(&stmt, i as i64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one row the way the desugar does, for the pure comparison tests.
    fn row(id: i64, kind: &str, name: &str, body: &str) -> DriverRow {
        DriverRow {
            id,
            kind: kind.to_string(),
            name: name.to_string(),
            base_url: None,
            auth: None,
            pagination: None,
            of_type: None,
            verb: None,
            body: Some(body.to_string()),
            irreversible: false,
            pushdown: None,
        }
    }

    fn driver_row(id: i64, name: &str, base_url: &str) -> DriverRow {
        DriverRow {
            base_url: Some(base_url.to_string()),
            ..row(id, "driver", name, "")
        }
    }

    fn shipped(label: &str, rows: Vec<DriverRow>) -> Vec<(String, Vec<DriverRow>)> {
        vec![(label.to_string(), rows)]
    }

    #[test]
    fn a_freshly_installed_declaration_reports_current() {
        let ship = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY-1"),
        ];
        let reports = reports_from(&ship, &shipped("acme.qfs", ship.clone()));
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, Status::Current);
        assert_eq!(reports[0].shipped.as_deref(), Some("acme.qfs"));
        assert!(reports[0].differs.is_empty());
    }

    #[test]
    fn a_declaration_whose_shipped_text_then_changed_reports_stale_and_names_the_statement() {
        // The Chatwork case that provoked the ticket: one view body corrected upstream.
        let installed = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY-1"),
            row(3, "view", "/acme/others", "SAME"),
        ];
        let ship = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY-2"),
            row(3, "view", "/acme/others", "SAME"),
        ];
        let reports = reports_from(&installed, &shipped("acme.qfs", ship));
        assert_eq!(reports[0].status, Status::Stale);
        assert_eq!(reports[0].differs, vec!["view /acme/things".to_string()]);
    }

    #[test]
    fn a_locally_authored_declaration_reports_local_never_stale() {
        // 「推測するな、宣言して拒否せよ」 — a name the binary ships nothing for is a different
        // FACT from an out-of-date one, and reporting it as stale would send the operator looking
        // for an upgrade that does not exist.
        let installed = vec![driver_row(1, "mine", "https://mine.test")];
        let ship = vec![driver_row(1, "acme", "https://acme.test")];
        let reports = reports_from(&installed, &shipped("acme.qfs", ship));
        assert_eq!(reports[0].status, Status::Local);
        assert_eq!(reports[0].shipped, None);
        assert!(reports[0].differs.is_empty());
    }

    #[test]
    fn a_re_install_returns_a_stale_declaration_to_current() {
        // A re-install APPENDS a newer row on the same key; the older one stays in the table. The
        // comparison must read what is live (newest per key), or a re-install would never clear.
        let ship = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY-2"),
        ];
        let installed = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY-1"),
            row(9, "view", "/acme/things", "BODY-2"),
        ];
        let reports = reports_from(&installed, &shipped("acme.qfs", ship));
        assert_eq!(reports[0].status, Status::Current);
    }

    #[test]
    fn a_statement_removed_upstream_leaves_a_row_that_reports_stale() {
        // The case the ticket flags as unhandled by superseding, and the reason identity is derived
        // rather than stamped: nothing supersedes a statement that vanished, so a stamp would still
        // read "current" while the registry carries a view the shipped declaration no longer has.
        let installed = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY"),
            row(3, "view", "/acme/retired", "OLD"),
        ];
        let ship = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "BODY"),
        ];
        let reports = reports_from(&installed, &shipped("acme.qfs", ship));
        assert_eq!(reports[0].status, Status::Stale);
        assert_eq!(reports[0].differs, vec!["view /acme/retired".to_string()]);
    }

    #[test]
    fn install_order_is_not_identity() {
        // Two installations that declare the same statements in different order are the same
        // declaration; only an item's CONTENT decides.
        let ship = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/a", "A"),
            row(3, "view", "/acme/b", "B"),
        ];
        let installed = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/b", "B"),
            row(3, "view", "/acme/a", "A"),
        ];
        assert_eq!(
            reports_from(&installed, &shipped("acme.qfs", ship))[0].status,
            Status::Current
        );
    }

    #[test]
    fn a_type_row_belongs_to_its_driver_and_counts_as_identity() {
        // The Chatwork fix that provoked the ticket added FIELDS to a `CREATE TYPE`. A comparison
        // that read only driver/view/map rows would call that installation current, which is the
        // exact untruth this node exists to prevent.
        let installed = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "type", "/type/acme/thing", "COLS-1"),
        ];
        let ship = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "type", "/type/acme/thing", "COLS-2"),
        ];
        let reports = reports_from(&installed, &shipped("acme.qfs", ship));
        assert_eq!(reports[0].status, Status::Stale);
        assert_eq!(
            reports[0].differs,
            vec!["type /type/acme/thing".to_string()]
        );
    }

    #[test]
    fn one_drivers_declaration_never_reads_anothers_rows() {
        let installed = vec![
            driver_row(1, "acme", "https://acme.test"),
            row(2, "view", "/acme/things", "A"),
            driver_row(3, "other", "https://other.test"),
            row(4, "view", "/other/things", "DIFFERENT"),
        ];
        let ship = vec![(
            "acme.qfs".to_string(),
            vec![
                driver_row(1, "acme", "https://acme.test"),
                row(2, "view", "/acme/things", "A"),
            ],
        )];
        let reports = reports_from(&installed, &ship);
        assert_eq!(reports.len(), 2, "one row per installed declaration");
        assert_eq!(reports[0].driver, "acme");
        assert_eq!(reports[0].status, Status::Current);
        assert_eq!(reports[1].driver, "other");
        assert_eq!(reports[1].status, Status::Local);
    }

    #[test]
    fn every_shipped_declaration_is_current_against_itself() {
        // The ratchet: the shipped side is rebuilt from the embedded bytes through the PRODUCTION
        // splitter + grammar, so an asset this binary can no longer desugar — or a comparison that
        // reads a column the desugar does not write — fails here rather than telling every operator
        // their up-to-date installation is stale.
        for asset in qfs_skill::DECLARED_DRIVERS {
            let rows = rows_from_script(asset.source);
            assert!(
                rows.iter().any(|r| r.kind == "driver"),
                "{} desugars to a driver row",
                asset.label
            );
            let shipped: Vec<(String, Vec<DriverRow>)> =
                vec![(asset.label.to_string(), rows.clone())];
            for report in reports_from(&rows, &shipped) {
                assert_eq!(
                    report.status,
                    Status::Current,
                    "{} compares current against itself (differs: {:?})",
                    asset.label,
                    report.differs
                );
            }
        }
    }

    /// End-to-end over the REAL seams: the shipped Chatwork declaration desugared through the
    /// production grammar, written into a `sys_drivers` table with the production DDL, read back
    /// through the production row reader, and compared. A freshly installed operator reports
    /// `current`; drift one stored body and the same operator reports `stale` naming that view.
    #[test]
    fn a_real_installation_of_the_shipped_chatwork_declaration_reports_current_then_stale() {
        let asset = qfs_skill::DECLARED_DRIVERS
            .iter()
            .find(|a| a.label == "chatwork.qfs")
            .expect("the binary ships the chatwork declaration");
        let shipped_rows = rows_from_script(asset.source);
        assert!(
            shipped_rows.len() > 5,
            "the shipped chatwork program desugars to its statements: {}",
            shipped_rows.len()
        );

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sys_drivers (id INTEGER PRIMARY KEY, kind TEXT, name TEXT, \
                 base_url TEXT, auth TEXT, pagination TEXT, of_type TEXT, verb TEXT, body TEXT, \
                 irreversible INTEGER NOT NULL DEFAULT 0, pushdown TEXT);",
        )
        .unwrap();
        let install = |conn: &rusqlite::Connection, r: &DriverRow| {
            conn.execute(
                "INSERT INTO sys_drivers \
                     (kind, name, base_url, auth, pagination, of_type, verb, body, irreversible, \
                      pushdown) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    r.kind,
                    r.name,
                    r.base_url,
                    r.auth,
                    r.pagination,
                    r.of_type,
                    r.verb,
                    r.body,
                    i64::from(r.irreversible),
                    r.pushdown,
                ],
            )
            .unwrap();
        };
        for r in &shipped_rows {
            install(&conn, r);
        }

        let ship = vec![("chatwork.qfs".to_string(), shipped_rows.clone())];
        let installed = crate::declared_driver::rows_from_conn(&conn).unwrap();
        let reports = reports_from(&installed, &ship);
        let chatwork = reports
            .iter()
            .find(|r| r.driver == "chatwork")
            .expect("the installation reports on chatwork");
        assert_eq!(
            chatwork.status,
            Status::Current,
            "differs: {:?}",
            chatwork.differs
        );

        // Now the operator's installation drifts: one stored view body no longer matches the
        // shipped text. This is the Chatwork `force=1` correction in miniature.
        let drifted = shipped_rows
            .iter()
            .find(|r| r.kind == "view")
            .expect("the declaration declares a view")
            .clone();
        conn.execute(
            "UPDATE sys_drivers SET body = 'DRIFTED' WHERE kind = 'view' AND name = ?1",
            rusqlite::params![drifted.name],
        )
        .unwrap();
        let installed = crate::declared_driver::rows_from_conn(&conn).unwrap();
        let reports = reports_from(&installed, &ship);
        let chatwork = reports.iter().find(|r| r.driver == "chatwork").unwrap();
        assert_eq!(chatwork.status, Status::Stale);
        assert_eq!(chatwork.differs, vec![format!("view {}", drifted.name)]);
        assert_eq!(chatwork.shipped.as_deref(), Some("chatwork.qfs"));
    }

    #[test]
    fn the_shipped_corpus_declares_the_names_the_cookbook_teaches() {
        // A declaration whose name the corpus does not carry would report `local` for every
        // operator running it — the failure mode is silent, so it is pinned here.
        let names: Vec<String> = qfs_skill::DECLARED_DRIVERS
            .iter()
            .flat_map(|a| rows_from_script(a.source))
            .filter(|r| r.kind == "driver")
            .map(|r| r.name)
            .collect();
        for expected in ["chatwork", "cloudflare", "slack"] {
            assert!(
                names.iter().any(|n| n == expected),
                "the shipped corpus declares {expected}: {names:?}"
            );
        }
    }
}
