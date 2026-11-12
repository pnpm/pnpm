//! Records under the `pnpr::audit` tracing target. Their actions and fields
//! are an operator-facing contract, listed in `pnpr/docs/configuration.md`.

use pnpr_policy::Identity;

/// Who performed an audited action. `id` stays the same when the account is
/// renamed, so it is what attributes an action. `name` is the account's
/// name at the time of the action.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Actor<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
}

impl<'a> Actor<'a> {
    /// No account: an unauthenticated caller, or pnpr itself.
    pub(crate) const NONE: Actor<'static> = Actor { id: "-", name: "-" };

    /// The SCIM client, which authenticates with the SCIM token.
    pub(crate) const SCIM: Actor<'static> = Actor { id: "-", name: "scim" };

    pub(crate) fn of(identity: &'a Identity) -> Self {
        match (identity.user_id(), identity.username()) {
            (Some(id), Some(name)) => Actor { id, name },
            _ => Actor::NONE,
        }
    }
}

/// Emit an audit record of `action` by `actor`, an [`Actor`], with the
/// action's own fields and message, as `tracing::info!` takes them.
macro_rules! audit {
    ($actor:expr, $action:expr, $($fields:tt)+) => {{
        let actor: $crate::audit::Actor<'_> = $actor;
        ::tracing::info!(
            target: "pnpr::audit",
            action = $action,
            actor = actor.name,
            actor_id = actor.id,
            $($fields)+
        )
    }};
}

pub(crate) use audit;
