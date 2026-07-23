//! The **request context** threaded to the read seam (`ReadDriver::scan`) so every driver
//! receives *who is asking* explicitly (the M2 principal seam).
//!
//! ## Why this is carried, not ambient
//! The mission ruling is deliberate: the principal is an **explicit argument** on the scan seam,
//! not a thread-local or an implicit default. That makes fail-closed a compile-time + test-pinned
//! fact — a driver cannot forget to consult it, and the anonymous default is the only thing a
//! caller can pass when it has resolved no actor. A widened default would fail a test.
//!
//! ## Secret-free by construction
//! [`Principal`] carries an owned user **id** only — never a session token, cookie, password, or
//! any credential. The richer authorization axes (roles/groups/memberships) are resolved on the
//! policy side into `qfs_server::DecisionContext`; the scan seam only needs the acting identity,
//! which is what the `/sys/whoami` face reads back.

/// The acting principal of a request: either a concrete authenticated user or the anonymous
/// (not-signed-in) actor. The signed-out state is a **first-class value**, not an error and not a
/// silent fallback to a sole user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Principal {
    /// No session resolved — the not-signed-in actor. Under it the policy gate sees no user, no
    /// roles, no groups, no memberships, so default-deny holds (fail closed).
    #[default]
    Anonymous,
    /// A concrete authenticated user, named by its owned id string (the binary maps a live
    /// `qfs-identity` `UserId` onto this). Never a credential.
    User(String),
}

impl Principal {
    /// The acting user id, or `None` for the anonymous actor. The policy side maps this onto a
    /// `DecisionContext` user; a consumer reads it back through `/sys/whoami`.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        match self {
            Principal::Anonymous => None,
            Principal::User(id) => Some(id.as_str()),
        }
    }

    /// Whether a concrete user is signed in (the negative is first-class — see [`Principal`]).
    #[must_use]
    pub fn is_signed_in(&self) -> bool {
        matches!(self, Principal::User(_))
    }
}

/// Everything the request layer resolved about the caller, frozen into owned, secret-free data and
/// threaded to the read seam. Today it carries the [`Principal`]; it is the seam a later milestone
/// widens (locale, tenant, trace id) without touching every driver signature again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestContext {
    /// Who is asking. [`Principal::Anonymous`] is the fail-closed default.
    pub principal: Principal,
}

impl RequestContext {
    /// The anonymous request context — no actor resolved. The fail-closed default every
    /// non-authenticated caller (the CLI, a cron committer, a request with no session) passes.
    #[must_use]
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// A request context for a concrete authenticated user id.
    #[must_use]
    pub fn for_user(id: impl Into<String>) -> Self {
        Self {
            principal: Principal::User(id.into()),
        }
    }

    /// The acting user id, or `None` when anonymous.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.principal.user()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_is_the_default_and_carries_no_user() {
        let ctx = RequestContext::anonymous();
        assert_eq!(ctx, RequestContext::default());
        assert_eq!(ctx.user(), None);
        assert!(!ctx.principal.is_signed_in());
    }

    #[test]
    fn a_user_context_carries_the_id_and_is_signed_in() {
        let ctx = RequestContext::for_user("1");
        assert_eq!(ctx.user(), Some("1"));
        assert!(ctx.principal.is_signed_in());
    }
}
