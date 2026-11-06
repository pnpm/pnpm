//! The reachable combinations of the `dedupePeerDependents` and
//! `projectScopedOptionalPeers` settings.

/// How peer-dependent variants are shared across workspace projects.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PeerDependentsDedupe {
    /// `dedupePeerDependents: false`. Optional peers are project-scoped.
    #[default]
    Off,
    /// `dedupePeerDependents: true`.
    On,
    /// `dedupePeerDependents: true` and `projectScopedOptionalPeers: true`.
    OnWithProjectScopedOptionalPeers,
}

/// Where a missing optional peer may come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionalPeerScope {
    /// Any version the workspace resolves.
    Workspace,
    /// Only versions the project's own dependencies resolve. A variant
    /// never collapses into one that resolves an optional peer it lacks.
    Project,
}

impl PeerDependentsDedupe {
    /// Whether peer-suffixed variants collapse after peer resolution and
    /// missing peers may be hoisted from other projects.
    pub(crate) fn dedupes(self) -> bool {
        match self {
            PeerDependentsDedupe::Off => false,
            PeerDependentsDedupe::On | PeerDependentsDedupe::OnWithProjectScopedOptionalPeers => {
                true
            }
        }
    }

    pub(crate) fn optional_peer_scope(self) -> OptionalPeerScope {
        match self {
            PeerDependentsDedupe::On => OptionalPeerScope::Workspace,
            PeerDependentsDedupe::Off | PeerDependentsDedupe::OnWithProjectScopedOptionalPeers => {
                OptionalPeerScope::Project
            }
        }
    }
}
