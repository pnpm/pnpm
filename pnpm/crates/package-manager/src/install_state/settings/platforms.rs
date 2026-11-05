//! The platforms an install recorded: the `supportedArchitectures` it ran
//! with and the platform it linked registry packages' variants for.

use pnpm_deps_restorer::platform_selection::{
    linked_platform_record, linked_platform_record_with_floors,
};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_workspace_state::WorkspaceState;

/// Whether the `supportedArchitectures` recorded by the last install
/// matches `live` (today's CLI-merged value). Read from the workspace
/// state for the frozen path's lockfile-up-to-date early return, whose
/// other guards (`wanted == current`, `.modules.yaml` consistency) cannot
/// see an architecture change: the skip set it would produce differs, so
/// the shortcut must not fire. A missing or unreadable state (`None`)
/// reports a match only when `live` is also unset — the conservative
/// direction is a full install.
fn recorded_supported_architectures_match(
    recorded: Option<&WorkspaceState>,
    live: Option<&SupportedArchitectures>,
) -> bool {
    let recorded =
        recorded.and_then(|state| state.settings.supported_architectures.as_ref());
    recorded
        == live
            .and_then(|value| serde_json::to_value(value).ok())
            .as_ref()
}

/// The platform this run links registry packages' variants for, as the
/// workspace state records it next to `recorded`: with the versions its
/// floors and ABI are matched against only when `recorded` carries them, so
/// the host is probed only for a lockfile with a variant that states one.
pub(super) fn live_linked_platform(
    recorded: Option<&str>,
    supported: Option<&SupportedArchitectures>,
) -> String {
    if recorded.is_some_and(|recorded| recorded.contains(' ')) {
        linked_platform_record_with_floors(supported)
    } else {
        linked_platform_record(supported)
    }
}

/// Whether the previous install recorded the `supportedArchitectures` this
/// run uses and linked registry packages' variants for the platform it
/// links. A missing record does not match.
pub(crate) fn recorded_platforms_match(
    recorded: Option<&WorkspaceState>,
    supported: Option<&SupportedArchitectures>,
) -> bool {
    let linked = recorded.and_then(|state| state.settings.linked_platform.as_deref());
    recorded_supported_architectures_match(recorded, supported)
        && linked.is_some_and(|linked| linked == live_linked_platform(Some(linked), supported))
}

pub(super) fn supported_architectures_json(
    supported: Option<&SupportedArchitectures>,
) -> Option<serde_json::Value> {
    supported.and_then(|value| serde_json::to_value(value).ok())
}
