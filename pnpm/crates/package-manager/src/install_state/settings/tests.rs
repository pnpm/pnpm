use super::{linked_platform_to_record, recorded_platforms_match};
use pnpm_deps_restorer::platform_selection::linked_platform_record;
use pnpm_lockfile::Lockfile;
use pnpm_workspace_state::WorkspaceState;
use std::path::Path;

const WITH_VARIANTS: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      native:
        specifier: 1.0.0
        version: 1.0.0
packages:
  native@1.0.0:
    resolution:
      type: variations
      variants:
        - resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
          targets:
            - any: true
        - resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
          targets:
            - os: linux
              cpu: x64
snapshots:
  native@1.0.0: {}
";

const WITHOUT_VARIANTS: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      plain:
        specifier: 1.0.0
        version: 1.0.0
packages:
  plain@1.0.0:
    resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==}
snapshots:
  plain@1.0.0: {}
";

fn lockfile(yaml: &str) -> Lockfile {
    Lockfile::parse(yaml, Path::new("pnpm-lock.yaml")).unwrap().unwrap()
}

fn recorded(linked_platform: Option<&str>) -> WorkspaceState {
    let mut state = WorkspaceState::default();
    state.settings.linked_platform = linked_platform.map(str::to_string);
    state
}

/// With `supportedArchitectures` unchanged, a `node_modules` with registry
/// variants that a previous install linked on another platform, such as a
/// glibc tree mounted into a musl container, is stale; one linked on this
/// host's platform is not.
#[test]
fn a_tree_linked_on_another_platform_is_stale() {
    let variants = lockfile(WITH_VARIANTS);
    let host = linked_platform_record(None);
    assert_eq!(linked_platform_to_record(&variants, None).as_deref(), Some(host.as_str()));
    assert!(recorded_platforms_match(Some(&recorded(Some(&host))), None, Some(&variants)));
    let other = if host.starts_with("aix-") { "linux-x64-musl" } else { "aix-ppc64" };
    assert!(!recorded_platforms_match(Some(&recorded(Some(other))), None, Some(&variants)));
    assert!(!recorded_platforms_match(Some(&recorded(None)), None, Some(&variants)));
}

/// A lockfile without registry variants records no linked platform, so a
/// state pnpm v11 wrote for it is current, and pnpm v11 and this pnpm can
/// share the project without reinstalling each other's tree.
#[test]
fn a_state_without_a_linked_platform_is_current_for_a_lockfile_without_variants() {
    let plain = lockfile(WITHOUT_VARIANTS);
    assert_eq!(linked_platform_to_record(&plain, None), None);
    assert!(recorded_platforms_match(Some(&recorded(None)), None, Some(&plain)));
}
