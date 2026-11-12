//! The registry graph requests route by, held as one swappable
//! [`RoutingSnapshot`]: the graph itself and everything the server builds
//! from it. With `registriesManagedBy: api` the metadata database stores the
//! graph, the admin API edits it, and every replica follows it.

pub(super) use admin::{if_match, is_registry_admin_path, registry_admin_routes, versioned};
pub(crate) use edit::{GraphEdit, edit_graph};
pub(super) use pin::with_pinned_routing;
pub(crate) use reload::{GraphState, reload_graph};
pub(crate) use snapshot::{RoutingSnapshot, UpstreamGuardContext, build_snapshot};
pub(crate) use stored::{
    Credentials, check_revision, graph_version, occupied_namespace_error, stored_document,
    stored_routing, with_stored_routing,
};

mod admin;
mod edit;
mod pin;
mod reload;
mod snapshot;
mod stored;
