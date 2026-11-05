use super::recorded_platforms_match;
use pnpm_deps_restorer::platform_selection::linked_platform_record;
use pnpm_workspace_state::WorkspaceState;

fn recorded(linked_platform: Option<&str>) -> WorkspaceState {
    let mut state = WorkspaceState::default();
    state.settings.linked_platform = linked_platform.map(str::to_string);
    state
}

/// With `supportedArchitectures` unchanged, a `node_modules` that a previous
/// install linked on another platform, such as a glibc tree mounted into a
/// musl container, is stale; one linked on this host's platform is not.
#[test]
fn a_tree_linked_on_another_platform_is_stale() {
    let host = linked_platform_record(None);
    assert!(recorded_platforms_match(Some(&recorded(Some(&host))), None));
    let other = if host.starts_with("aix-") { "linux-x64-musl" } else { "aix-ppc64" };
    assert!(!recorded_platforms_match(Some(&recorded(Some(other))), None));
    assert!(!recorded_platforms_match(Some(&recorded(None)), None));
}
