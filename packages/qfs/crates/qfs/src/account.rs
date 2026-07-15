//! ADR 0008 §3 — the **`qfs app` / `qfs account` composition root** (EPIC 20260702120000 /
//! ticket 20260702120040): the per-layer verbs that dissolve the `connection` grab-bag.
//!
//! - **`qfs app`** owns OAuth **app registrations** — the operator's client id/secret (today:
//!   Google's `credentials.json`), sealed in the vault under the `<provider>-app` driver exactly
//!   where the retired connection namespace used to put it (`crate::google::google_app_config`
//!   keeps reading it unchanged).
//! - **`qfs account`** owns external **service accounts** — the token + the recorded consent.
//!   For Google, ONE account-level authorization serves gmail + gdrive + ga (the shared
//!   `google:<email>:refresh_token`, the scope union, and the ADR-0008 incremental-auth fix): a
//!   terminal runs the live paste-back browser consent (print the URL, authorize in the LOCAL
//!   browser — works over plain SSH, no listener — paste the redirect back; the old
//!   `QFS_GOOGLE_CONSENT=1` opt-in is retired — `qfs account add google --app <label>` on a TTY
//!   *is* the opt-in), automation pipes a refresh token on stdin with the email as the label and
//!   `--app <label>`. Other cloud providers (github/slack/objstore/cf) and declared drivers pipe or
//!   prompt their token per label.
//!
//! ## Consent keying (ADR 0008 §4 — mount-bound)
//! Consent is recorded per Google DRIVER keyed by the ACCOUNT EMAIL (per `(provider, label)` for
//! the other clouds) — exactly the `(kind, account)` pair the commit-time bind gate consults for
//! a connect-created mount. There is no selection state: an authorized account becomes usable by
//! connecting a mount to it (`qfs connect /mail --driver gmail --account <email>`).
//!
//! ## Secret hygiene (blueprint §8)
//! Tokens arrive on stdin or an echo-off TTY prompt, never argv; they are sealed by the vault and
//! never printed back. `app list` / `account list` render selectors + metadata only.

use std::io::Read;
use std::sync::Arc;

use qfs_cmd::AccountAction;
use qfs_secrets::{is_cloud_driver, ConnectionId, CredentialKey, DriverId, Secret, Secrets};
use rusqlite::Connection;

use crate::connection::{open_project_conn, open_store, require_signed_in};
use crate::secret_store;

/// The Google provider's three drivers — one account authorization serves them all (the shared
/// refresh token; ADR 0008 §4 "one consent, many drivers").
const GOOGLE_DRIVERS: [&str; 3] = ["gmail", "gdrive", "ga"];

/// The injected app/account launcher. Returns the process exit code (`0` ok, `1` on a structured,
/// secret-free error). Never panics.
#[must_use]
pub fn run_account(action: &AccountAction) -> i32 {
    match run_inner(action) {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

fn run_inner(action: &AccountAction) -> Result<String, String> {
    match action {
        AccountAction::AppAdd { provider, label } => app_add(provider, label),
        AccountAction::AppList => app_list(),
        AccountAction::AppRemove { provider, label } => app_remove(provider, label),
        AccountAction::Add {
            provider,
            label,
            app,
        } => match provider.as_str() {
            "google" => add_google(label.as_deref(), app.as_deref()),
            other if is_token_account_provider(other) => {
                add_cloud(other, label.as_deref().unwrap_or("default"))
            }
            other => Err(format!(
                "`{other}` is not an account provider — accounts exist for google, github, slack, \
                 objstore, cf, and declared drivers. A local source (SQL file, git repo) needs no \
                 account: declare it with CREATE CONNECTION / `qfs connect`"
            )),
        },
        AccountAction::List => list_accounts(),
        // Shares `remove_account` with the `REMOVE /sys/accounts/…` statement path (one deletion).
        AccountAction::Remove { provider, label } => remove_account(provider, label),
        AccountAction::Rotate { provider, label } => rotate_account(provider, label),
        AccountAction::Revoke { provider, label } => revoke_account(provider, label),
    }
}

/// `qfs account rotate <provider> <label>` — re-mint the account's secret (t79, moved here from
/// the retired `connection` namespace): read a NEW secret from stdin, re-seal it, and clear any
/// revocation. The offboarding answer — replace, not un-grant.
fn rotate_account(provider: &str, label: &str) -> Result<String, String> {
    // A cloud account carries the same sign-in gate as `add` (a cloud credential is unusable for
    // an unauthenticated operator); resolve identity BEFORE touching stdin.
    if is_token_account_provider(provider) {
        let _ = require_signed_in(provider)?;
    }
    let value = read_secret(
        "new secret",
        &format!("printf %s \"$TOKEN\" | qfs account rotate {provider} {label}"),
    )?;
    let conn_id = ConnectionId::new(label).map_err(|e| e.to_string())?;
    let key = CredentialKey::new(DriverId(provider.to_string()), conn_id);
    let store = open_store()?;
    store
        .rotate(&key, Secret::from(value))
        .map_err(|e| format!("rotating the credential: {e}"))?;
    crate::connection::emit_connection_audit("ROTATE", &format!("{provider}/{label}"));
    Ok(format!(
        "rotated {provider}/{label} (secret re-minted; any revocation cleared)"
    ))
}

/// `qfs account revoke <provider> <label>` — mark the account's credential unresolvable (t79,
/// moved here from the retired `connection` namespace): a later bind fails closed (the secret is
/// never returned); other accounts keep working. Re-minting (`qfs account rotate`) restores use.
fn revoke_account(provider: &str, label: &str) -> Result<String, String> {
    let conn_id = ConnectionId::new(label).map_err(|e| e.to_string())?;
    let key = CredentialKey::new(DriverId(provider.to_string()), conn_id);
    let store = open_store()?;
    store
        .revoke(&key)
        .map_err(|e| format!("revoking the account: {e}"))?;
    crate::connection::emit_connection_audit("REVOKE", &format!("{provider}/{label}"));
    Ok(format!(
        "revoked {provider}/{label} (it can no longer resolve until re-minted with `qfs account rotate`)"
    ))
}

/// The `<provider>-app` driver id an app registration is sealed under (the same key the retired
/// connection namespace wrote, so `google_app_config` reads on unchanged).
fn app_key(provider: &str, label: &str) -> Result<CredentialKey, String> {
    if provider != "google" {
        return Err(format!(
            "no app registration exists for `{provider}` — today the OAuth-app layer serves \
             google (its Desktop-app credentials.json); other providers authenticate per account \
             token"
        ));
    }
    let conn = ConnectionId::new(label).map_err(|e| e.to_string())?;
    Ok(CredentialKey::new(
        DriverId(format!("{provider}-app")),
        conn,
    ))
}

/// `qfs app add google <label> < credentials.json` — seal the operator's OAuth app credentials.
fn app_add(provider: &str, label: &str) -> Result<String, String> {
    let key = app_key(provider, label)?;
    let value = read_secret(
        "app credentials",
        "cat credentials.json | qfs app add google home",
    )?;
    let store = open_store()?;
    store
        .put(&key, Secret::from(value))
        .map_err(|e| format!("storing the app credentials: {e}"))?;
    Ok(format!(
        "registered the {provider} OAuth app `{label}` (credentials sealed in the vault; `qfs account add \
         {provider} --app {label}` can now authorize accounts)"
    ))
}

/// `qfs app list` — the registered OAuth apps (provider + label + created_at; never a secret).
fn app_list() -> Result<String, String> {
    let store = open_store()?;
    let records = store
        .list(None)
        .map_err(|e| format!("listing app registrations: {e}"))?;
    let apps: Vec<String> = records
        .iter()
        .filter(|r| r.driver.as_str().ends_with("-app"))
        .map(|r| {
            let provider = r.driver.as_str().trim_end_matches("-app");
            format!("{provider}\t{}\tregistered {}", r.connection, r.created_at)
        })
        .collect();
    if apps.is_empty() {
        return Ok(
            "no OAuth apps registered — `cat credentials.json | qfs app add google home`"
                .to_string(),
        );
    }
    Ok(apps.join("\n"))
}

/// `qfs app remove <provider> <label>` — delete the app registration (accounts' tokens stay).
fn app_remove(provider: &str, label: &str) -> Result<String, String> {
    let key = app_key(provider, label)?;
    let store = open_store()?;
    store
        .remove(&key)
        .map_err(|e| format!("removing the app registration: {e}"))?;
    Ok(format!(
        "removed the {provider} OAuth app registration `{label}`"
    ))
}

/// `qfs account add google [email]` — authorize a Google account. On a terminal with no piped
/// token this runs the LIVE paste-back browser consent (print the consent URL, authorize in the
/// user's LOCAL browser, paste the redirect URL back — the documented non-hermetic seam; the old
/// `QFS_GOOGLE_CONSENT` env opt-in is retired; invoking this verb on a TTY is the opt-in);
/// automation pipes the refresh token with the email as the label.
fn add_google(label: Option<&str>, app: Option<&str>) -> Result<String, String> {
    let subject = require_signed_in("gmail")?;
    let store = open_store()?;
    let app = app.ok_or("google account authorization needs an app label: `qfs account add google --app <label> [email]`")?;

    let email = if crate::tty::stdin_is_terminal() {
        // Interactive: the real paste-back browser consent (requests the PROVIDER scope union —
        // one authorization serves gmail+gdrive+ga; persists the refresh token under the email).
        let store_arc: Arc<dyn Secrets> = Arc::new(store);
        crate::google::run_google_consent(store_arc, app)
            .map_err(|e| format!("google consent failed: {e}"))?
    } else {
        // Automation: the refresh token on stdin, the email as the label.
        let Some(email) = label else {
            return Err(
                "the token-import path needs the account email — `printf %s \
                        \"$REFRESH_TOKEN\" | qfs account add google you@example.com --app <label>`"
                    .into(),
            );
        };
        let token = read_secret(
            "refresh token",
            "printf %s \"$REFRESH_TOKEN\" | qfs account add google you@example.com --app qmu",
        )?;
        let key = qfs_google_auth::refresh_token_key(email).map_err(|e| e.to_string())?;
        store
            .put(&key, Secret::from(token))
            .map_err(|e| format!("storing the refresh token: {e}"))?;
        email.to_string()
    };

    // Record the account-level consent per Google DRIVER, keyed by the ACCOUNT EMAIL (ADR 0008
    // §4 — the mount carries the account, so the commit-time bind gate consults the mount's
    // `(driver, account)`). No selection is made: the account becomes usable by connecting a
    // mount to it (`qfs connect /mail --driver gmail --account <email>`).
    let proj = open_project_conn()?;
    record_google_consents(&proj, &subject, &email, app)?;
    Ok(format!(
        "authorized google account {email} (one authorization serves mail, drive, and analytics; \
         consent granted by {subject} through app {app}) — mount it with `qfs connect /mail --driver gmail --account {email}`"
    ))
}

/// Consent rows for the three Google drivers, keyed by the account email — what the mount-bound
/// bind gate consults for a `(kind, account)` cloud mount (see the module doc).
fn record_google_consents(
    proj: &Connection,
    subject: &str,
    email: &str,
    app: &str,
) -> Result<(), String> {
    for driver in GOOGLE_DRIVERS {
        secret_store::db_record_consent_with_app(
            proj,
            driver,
            email,
            subject,
            google_scope(driver),
            Some(app),
        )
        .map_err(|e| format!("recording consent for {driver}: {e}"))?;
    }
    Ok(())
}

/// The §10 consent-scope hint recorded per Google driver (metadata; the live token negotiation is
/// the OAuth client's).
fn google_scope(driver: &str) -> &'static str {
    match driver {
        "gmail" => "gmail.modify gmail.compose",
        "gdrive" => "drive",
        _ => "analytics.readonly",
    }
}

/// `qfs account add <provider> [label]` for non-Google token-backed account providers: the token on
/// stdin (or an echo-off TTY prompt), sealed under `(provider, label)`, with the consent recorded.
fn add_cloud(provider: &str, label: &str) -> Result<String, String> {
    let subject = require_signed_in(provider)?;
    let token = if crate::tty::stdin_is_terminal() {
        crate::tty::prompt_secret(&format!("{provider} token (input hidden): "))?
            .expose_str()
            .ok_or("the token is not valid UTF-8")?
            .to_string()
    } else {
        read_secret(
            "token",
            &format!("printf %s \"$TOKEN\" | qfs account add {provider} {label}"),
        )?
    };
    let conn_id = ConnectionId::new(label).map_err(|e| e.to_string())?;
    let key = CredentialKey::new(DriverId(provider.to_string()), conn_id);
    let store = open_store()?;
    store
        .put(&key, Secret::from(token))
        .map_err(|e| format!("storing the token: {e}"))?;
    let proj = open_project_conn()?;
    secret_store::db_record_consent(&proj, provider, label, &subject, "")
        .map_err(|e| format!("recording consent: {e}"))?;
    Ok(format!(
        "authorized {provider} account `{label}` (consent granted by {subject})"
    ))
}

/// `qfs account list` — the authorized service accounts (provider + label + created_at; never a
/// token). Google accounts render their decoded email.
fn list_accounts() -> Result<String, String> {
    let store = open_store()?;
    let records = store
        .list(None)
        .map_err(|e| format!("listing accounts: {e}"))?;
    let accounts: Vec<String> = records
        .iter()
        .filter_map(|r| {
            let driver = r.driver.as_str();
            if driver == "google" {
                let email = qfs_google_auth::decode_account_email(r.connection.as_str());
                Some(format!("google\t{email}\tauthorized {}", r.created_at))
            } else if is_token_account_provider(driver) {
                Some(format!(
                    "{driver}\t{}\tauthorized {}",
                    r.connection.as_str(),
                    r.created_at
                ))
            } else {
                None
            }
        })
        .collect();
    if accounts.is_empty() {
        return Ok(
            "no service accounts yet — `qfs account add google --app <label>` (or github/slack/…)"
                .to_string(),
        );
    }
    Ok(accounts.join("\n"))
}

/// `qfs account remove google <email>` — delete the refresh token and the three drivers' consent
/// rows (data-sovereignty: deletion is first-class and complete). Mounts bound to the account
/// stay defined and fail closed until reconnected to another account.
fn remove_google(email: &str) -> Result<String, String> {
    let key = qfs_google_auth::refresh_token_key(email).map_err(|e| e.to_string())?;
    let store = open_store()?;
    store
        .remove(&key)
        .map_err(|e| format!("removing the refresh token: {e}"))?;
    let proj = open_project_conn()?;
    for driver in GOOGLE_DRIVERS {
        delete_consent(&proj, driver, email)?;
    }
    Ok(format!(
        "removed google account {email} (token and consents deleted)"
    ))
}

/// `qfs account remove <provider> <label>` — delete the token + the consent row.
fn remove_cloud(provider: &str, label: &str) -> Result<String, String> {
    let conn_id = ConnectionId::new(label).map_err(|e| e.to_string())?;
    let key = CredentialKey::new(DriverId(provider.to_string()), conn_id);
    let store = open_store()?;
    store
        .remove(&key)
        .map_err(|e| format!("removing the token: {e}"))?;
    let proj = open_project_conn()?;
    delete_consent(&proj, provider, label)?;
    Ok(format!(
        "removed {provider} account `{label}` (token and consent deleted)"
    ))
}

// ---- The shared consent surface behind BOTH the CLI and the `CREATE ACCOUNT` statement ----------
//
// 20260703040000 (the in-language account declaration): `CREATE ACCOUNT` and `qfs account add` write
// the SAME `connection_consent` state through the SAME writer (one source of truth), and `REMOVE
// /sys/accounts/…` and `qfs account remove` delete through the same path. The statement path records
// consent + enforces the operator gate; the token VALUE stays out-of-band (never in a statement).

/// Record an account's consent — Google's three drivers (one consent, many drivers; ADR 0008 §4) or
/// one cloud row. The single writer behind `qfs account add`'s consent step AND the `CREATE ACCOUNT`
/// statement. Does NOT seal a token (that stays out-of-band).
pub(crate) fn record_account_consent(
    proj: &Connection,
    provider: &str,
    account: &str,
    subject: &str,
    app: Option<&str>,
) -> Result<(), String> {
    if provider == "google" {
        let app = app.ok_or("google account declarations need APP '<label>'")?;
        record_google_consents(proj, subject, account, app)
    } else {
        secret_store::db_record_consent(proj, provider, account, subject, "")
            .map_err(|e| format!("recording consent: {e}"))
    }
}

/// Whether `provider` names a token-backed account provider: compiled cloud drivers plus declared
/// drivers installed through `/sys/drivers`.
fn is_token_account_provider(provider: &str) -> bool {
    provider == "chatwork"
        || is_cloud_driver(&DriverId(provider.to_string()))
        || is_declared_driver_provider(provider)
}

/// Whether `provider` is an installed declared driver. Declared drivers can use the same encrypted
/// vault as compiled cloud drivers through `SECRET 'vault:<provider>/<label>'`.
fn is_declared_driver_provider(provider: &str) -> bool {
    if crate::declared_driver::load_declared_drivers()
        .iter()
        .any(|d| d.name == provider)
    {
        return true;
    }
    let Ok(conn) = open_project_conn() else {
        return false;
    };
    crate::path_binding::db_list_bindings(&conn)
        .unwrap_or_default()
        .iter()
        .any(|b| {
            b.alias_of.is_none()
                && b.driver_id.as_deref() == Some(provider)
                && crate::describe::cred_free_driver(provider).is_none()
        })
}

/// Whether `provider` names a known service-account provider (the same set `qfs account add`
/// accepts): google + token-backed account providers. A local source (SQL file, git repo) needs no
/// account.
fn is_account_provider(provider: &str) -> bool {
    provider == "google" || is_token_account_provider(provider)
}

/// The `CREATE ACCOUNT <provider> '<account>'` apply (20260703040000): enforce the signed-in-operator
/// gate (the recorded `subject`), then record consent through the shared writer. The token VALUE is
/// NOT sealed here — it stays out-of-band (stdin import / paste-back consent), by rule never in a
/// statement. Returns a human-facing confirmation.
///
/// # Errors
/// A secret-free message if the provider is not an account provider, no operator is signed in (the
/// t54 gate), or the Project DB write fails.
pub(crate) fn declare_account(
    provider: &str,
    account: &str,
    app: Option<&str>,
) -> Result<String, String> {
    if !is_account_provider(provider) {
        return Err(format!(
            "`{provider}` is not an account provider — accounts exist for google, github, slack, \
             objstore, cf, and declared drivers. A local source (SQL file, git repo) needs no \
             account: declare it with CREATE CONNECTION / `qfs connect`"
        ));
    }
    // The t54 gate: recording consent needs a signed-in operator (the `subject`). Resolved BEFORE any
    // write — a declaration by an unauthenticated operator is refused, exactly as `qfs account add`.
    let subject = require_signed_in(provider)?;
    let proj = open_project_conn()?;
    record_account_consent(&proj, provider, account, &subject, app)?;
    Ok(format!(
        "declared {provider} account `{account}` (consent granted by {subject}; seal the token \
         out-of-band with `qfs account add {provider}`)"
    ))
}

/// The `REMOVE /sys/accounts/<provider>/<account>` apply (20260703040000): delete the sealed token
/// AND the consent row(s) — the complete-deletion contract of `qfs account remove` (data
/// sovereignty). Shares the CLI removal exactly.
///
/// # Errors
/// A secret-free message on an I/O failure.
pub(crate) fn remove_account(provider: &str, account: &str) -> Result<String, String> {
    if provider == "google" {
        remove_google(account)
    } else {
        remove_cloud(provider, account)
    }
}

/// Delete one consent row (the t54 ledger keeps history via the audit chain; the LIVE row gates
/// binds, so a removed account must not keep gating open).
fn delete_consent(proj: &Connection, driver: &str, connection: &str) -> Result<(), String> {
    proj.execute(
        "DELETE FROM connection_consent WHERE driver = ?1 AND connection = ?2",
        rusqlite::params![driver, connection],
    )
    .map_err(|e| format!("deleting consent for {driver}: {e}"))?;
    Ok(())
}

/// Read a single secret value from stdin, never argv (mirrors `connection.rs`'s reader).
fn read_secret(what: &str, example: &str) -> Result<String, String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("reading the {what} from stdin: {e}"))?;
    let value = buf.trim_end_matches(['\n', '\r']).to_string();
    if value.is_empty() {
        return Err(format!("no {what} on stdin — pipe it, e.g. `{example}`"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_fresh_home<T>(f: impl FnOnce() -> T) -> T {
        let _home = crate::testenv::HomeGuard::with_passphrase("account-test-pass");
        f()
    }

    /// The Google token-import bookkeeping: the refresh token lands under the account key and all
    /// three Google drivers get a consent row keyed by the ACCOUNT EMAIL (ADR 0008 — what the
    /// mount-bound bind gate consults). No selection state exists (migration #11 dropped it).
    /// Removal deletes all of it (deletion is complete).
    #[test]
    fn google_account_bookkeeping_round_trips() {
        with_fresh_home(|| {
            // Seed the pieces `add_google`'s non-stdin internals write (the stdin read itself is
            // exercised by the release smoke, not in-process).
            let store = open_store().unwrap();
            let key = qfs_google_auth::refresh_token_key("you@example.com").unwrap();
            store.put(&key, Secret::from("1//refresh")).unwrap();
            let proj = open_project_conn().unwrap();
            record_google_consents(&proj, "op@example.com", "you@example.com", "client").unwrap();

            for driver in GOOGLE_DRIVERS {
                assert!(
                    secret_store::db_get_consent(&proj, driver, "you@example.com").is_some(),
                    "{driver} consent recorded under the account email"
                );
                assert_eq!(
                    secret_store::db_get_consent_app(&proj, driver, "you@example.com").as_deref(),
                    Some("client")
                );
            }
            drop(proj);

            let out = remove_google("you@example.com").unwrap();
            assert!(out.contains("you@example.com"));
            let proj = open_project_conn().unwrap();
            for driver in GOOGLE_DRIVERS {
                assert!(
                    secret_store::db_get_consent(&proj, driver, "you@example.com").is_none(),
                    "{driver} consent deleted"
                );
            }
        });
    }

    /// Seed a sole signed-in operator so `require_signed_in` resolves (the CLI's `qfs init` twin).
    fn sign_in(email: &str) {
        use qfs_identity::IdentityStore as _;
        crate::identity::open_identity_store()
            .unwrap()
            .create_user(email)
            .unwrap();
    }

    fn seed_declared_chatwork_driver() {
        let sys = crate::store::open_system_db()
            .unwrap()
            .expect("system db opens");
        let conn = sys.into_db().into_connection();
        conn.execute(
            "INSERT INTO sys_drivers \
                 (kind, name, base_url, auth, pagination, of_type, verb, body, irreversible) \
             VALUES ('driver', 'chatwork', 'https://api.chatwork.com/v2', \
                     '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}', NULL, NULL, NULL, NULL, 0)",
            [],
        )
        .unwrap();
    }

    /// 20260703040000: `CREATE ACCOUNT` (via `declare_account`) records consent under the signed-in
    /// operator — Google's THREE driver rows keyed by the email, a cloud provider's ONE row — the
    /// SAME `connection_consent` state `qfs account add` writes (one shared writer). No token is
    /// sealed (that stays out-of-band).
    #[test]
    fn declare_account_records_consent_under_the_signed_in_operator() {
        with_fresh_home(|| {
            sign_in("op@example.com");

            let msg = declare_account("google", "you@example.com", Some("client")).unwrap();
            assert!(msg.contains("consent granted by op@example.com"), "{msg}");
            let proj = open_project_conn().unwrap();
            for driver in GOOGLE_DRIVERS {
                let consent = secret_store::db_get_consent(&proj, driver, "you@example.com")
                    .expect("google consent recorded per driver");
                assert_eq!(consent.subject, "op@example.com", "operator is the subject");
                assert_eq!(
                    secret_store::db_get_consent_app(&proj, driver, "you@example.com").as_deref(),
                    Some("client")
                );
            }
            // No token was sealed — the statement records consent only (declare→seal is two steps).
            let store = open_store().unwrap();
            assert!(
                store
                    .get(&qfs_google_auth::refresh_token_key("you@example.com").unwrap())
                    .is_err(),
                "declaring an account seals no token (out-of-band by rule)"
            );

            declare_account("github", "work", None).unwrap();
            assert!(
                secret_store::db_get_consent(&proj, "github", "work").is_some(),
                "a cloud provider records one consent row"
            );
        });
    }

    #[test]
    fn declared_driver_is_a_token_account_provider() {
        with_fresh_home(|| {
            sign_in("op@example.com");
            seed_declared_chatwork_driver();

            assert!(
                is_token_account_provider("chatwork"),
                "installed declared drivers accept account tokens"
            );
            let proj = open_project_conn().unwrap();
            record_account_consent(&proj, "chatwork", "work", "op@example.com", None).unwrap();
            assert!(
                secret_store::db_get_consent(&proj, "chatwork", "work").is_some(),
                "declared drivers record consent like token-backed cloud providers"
            );
            let key = CredentialKey::new(
                DriverId("chatwork".into()),
                ConnectionId::new("work").unwrap(),
            );
            let store = open_store().unwrap();
            store.put(&key, Secret::from("cw-token")).unwrap();

            let listed = list_accounts().unwrap();
            assert!(listed.contains("chatwork\twork"), "{listed}");
        });
    }

    /// The t54 gate on the STATEMENT path: recording consent needs a signed-in operator (the
    /// `subject`), so `CREATE ACCOUNT` on a host with no identity fails closed — the same gate
    /// `qfs account add` enforces (unlike CONNECT's ungated `/sys/paths` write).
    #[test]
    fn declare_account_refuses_without_a_signed_in_operator() {
        with_fresh_home(|| {
            let err = declare_account("google", "you@example.com", Some("client")).unwrap_err();
            assert!(
                err.contains("sign") || err.contains("qfs init"),
                "the gate asks for sign-in: {err}"
            );
            // And nothing was written (fail closed BEFORE any consent row).
            let proj = open_project_conn().unwrap();
            assert!(
                secret_store::db_get_consent(&proj, "gmail", "you@example.com").is_none(),
                "a refused declaration records no consent"
            );
        });
    }

    /// A non-provider (a local source) is an actionable error — the same set `qfs account add`
    /// accepts. Validated BEFORE the gate, so it needs no signed-in operator.
    #[test]
    fn declare_account_rejects_a_non_provider() {
        with_fresh_home(|| {
            let err = declare_account("sqlite", "x", None).unwrap_err();
            assert!(err.contains("not an account provider"), "{err}");
            assert!(err.contains("CREATE CONNECTION"), "actionable: {err}");
        });
    }

    /// 20260703040000: `REMOVE /sys/accounts/<provider>/<account>` (via `remove_account`) deletes the
    /// token AND the consent — the SAME complete deletion `qfs account remove` does.
    #[test]
    fn remove_account_deletes_token_and_consent() {
        with_fresh_home(|| {
            // Seed a github account (token + consent), then remove it in-language.
            let conn_id = ConnectionId::new("work").unwrap();
            let key = CredentialKey::new(DriverId("github".into()), conn_id);
            let store = open_store().unwrap();
            store.put(&key, Secret::from("ghp_token")).unwrap();
            let proj = open_project_conn().unwrap();
            record_account_consent(&proj, "github", "work", "op@example.com", None).unwrap();
            drop(proj);

            remove_account("github", "work").unwrap();

            let store = open_store().unwrap();
            assert!(store.get(&key).is_err(), "token deleted");
            let proj = open_project_conn().unwrap();
            assert!(
                secret_store::db_get_consent(&proj, "github", "work").is_none(),
                "consent deleted"
            );
        });
    }

    /// 20260703040000: the `/sys/accounts` backend adapter — `record_account`/`remove_account` (the
    /// desugar-target of `CREATE ACCOUNT` / `REMOVE /sys/accounts/…`) extract `(provider, account)`
    /// and drive the SHARED account logic (gate + consent writer / complete deletion), so the
    /// statement path and the CLI write one state. The real `SystemDbBackend` + `declare_account`
    /// share the same XDG home DBs here.
    #[test]
    fn sys_accounts_backend_adapter_shares_the_account_logic() {
        use qfs_driver_sys::SysBackend as _;
        with_fresh_home(|| {
            sign_in("op@example.com");
            let backend = crate::sys::SystemDbBackend::open_default().expect("backend opens");

            let row = qfs_core::RowBatch::new(
                qfs_core::Schema::new(vec![
                    qfs_core::Column::new("provider", qfs_core::ColumnType::Text, false),
                    qfs_core::Column::new("account", qfs_core::ColumnType::Text, false),
                ]),
                vec![qfs_core::Row::new(vec![
                    qfs_core::Value::Text("github".into()),
                    qfs_core::Value::Text("work".into()),
                ])],
            );
            backend
                .record_account(&row)
                .expect("record_account applies");
            assert!(
                secret_store::db_get_consent(&open_project_conn().unwrap(), "github", "work")
                    .is_some(),
                "the backend adapter recorded consent through declare_account"
            );

            backend
                .remove_account("github", "work")
                .expect("remove_account applies");
            assert!(
                secret_store::db_get_consent(&open_project_conn().unwrap(), "github", "work")
                    .is_none(),
                "the backend adapter deleted consent through remove_account"
            );
        });
    }

    /// An unknown provider is an actionable error naming the cloud set; an app registration for a
    /// non-google provider is refused (only google has an OAuth-app layer today).
    #[test]
    fn unknown_providers_are_actionable_errors() {
        with_fresh_home(|| {
            let err = run_inner(&AccountAction::Add {
                provider: "sqlite".into(),
                label: None,
                app: None,
            })
            .unwrap_err();
            assert!(err.contains("not an account provider"), "{err}");
            assert!(err.contains("CREATE CONNECTION"), "actionable: {err}");
            let err = app_key("github", "default").unwrap_err();
            assert!(err.contains("github"), "{err}");
        });
    }

    /// `app add` → `app list` → `app remove` round-trips a labeled google-app registration under
    /// the SAME driver `google_app_config` reads.
    #[test]
    fn app_registration_round_trips_under_a_label() {
        with_fresh_home(|| {
            let store = open_store().unwrap();
            let key = app_key("google", "client").unwrap();
            assert_eq!(key.driver.as_str(), "google-app");
            assert_eq!(key.connection.as_str(), "client");
            store.put(&key, Secret::from("{\"installed\":{}}")).unwrap();
            drop(store);

            let listed = app_list().unwrap();
            assert!(listed.contains("google"), "{listed}");
            assert!(listed.contains("client"), "{listed}");
            let removed = app_remove("google", "client").unwrap();
            assert!(removed.contains("google"));
            let listed = app_list().unwrap();
            assert!(listed.contains("no OAuth apps"), "{listed}");
        });
    }

    #[test]
    fn cf_account_list_shows_label_without_secret_value() {
        use qfs_secrets::{ConnectionId, CredentialKey, DriverId, Secret};

        with_fresh_home(|| {
            let credential_value = time::OffsetDateTime::now_utc()
                .unix_timestamp_nanos()
                .to_string();
            let store = open_store().unwrap();
            let key = CredentialKey::new(
                DriverId("cf".to_string()),
                ConnectionId::new("mycf").unwrap(),
            );
            store
                .put(&key, Secret::from(credential_value.clone()))
                .unwrap();
            drop(store);

            let listed = list_accounts().unwrap();

            assert!(listed.contains("cf\tmycf\tauthorized"), "{listed}");
            assert!(!listed.contains(&credential_value), "{listed}");
        });
    }
}
