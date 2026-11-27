//! The kinds of file a full install reads in the workspace outside
//! `node_modules`. `repeat_install_read_set` sorts every file a full install
//! opens into one of them, and the oracle runs an edit scenario for each.

/// A kind of file a full install reads in the workspace outside
/// `node_modules`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumIter)]
pub enum Input {
    /// A project's manifest, recorded in `manifest_files`.
    Manifest,
    /// A lockfile, recorded in `files`.
    Lockfile,
    /// A pnpmfile, recorded in `pnpmfiles`.
    Pnpmfile,
    /// A patch file `patchedDependencies` names, recorded in `files`.
    PatchFile,
    /// A local tarball dependency, recorded in `local_dependencies`.
    LocalTarball,
    /// A file in a local directory dependency that is not a workspace
    /// project. `local_dependencies` records the directory.
    LocalDirectory,
    /// `.npmrc`.
    Npmrc,
    /// `pnpm-workspace.yaml`.
    WorkspaceManifest,
    /// A directory project discovery lists.
    ProjectDirectories,
}

use super::{
    Layout,
    Scenario,
};
use serde_json::json;

const SHARED: Layout =
    Layout { hoisted: false, shared_lockfile: true, inject_workspace_packages: false };

#[test]
fn runtime_on_fail_overrides_the_declared_runtime() {
    let mut scenario = Scenario::new(SHARED);
    scenario.set_workspace_setting("runtimeOnFail", Some("warn"));
    let mut manifest = json!({
        "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        "devEngines": {
            "runtime": { "name": "node", "version": "24.22.3", "onFail": "download" }
        }
    });
    scenario.write_manifest("a", manifest.clone());
    scenario.install(&[]);
    scenario.check_gates();
    manifest["description"] = json!("edited");
    scenario.write_manifest("a", manifest);
    scenario.check_gates();
}

#[test]
fn trust_lockfile_overrides_the_workspace_setting() {
    let scenario = Scenario::new(SHARED);
    scenario.set_workspace_setting("trustLockfile", Some("false"));
    scenario.install(&["--trust-lockfile"]);
    let state =
        || pnpm_workspace_state::load_workspace_state(scenario.workspace()).unwrap().unwrap();
    assert_eq!(state().settings.trust_lockfile, Some(true));
    assert!(scenario.exec(&["a"], "error"));
    assert_eq!(state().settings.trust_lockfile, Some(true));
    assert!(!scenario.install(&[]));
    assert_eq!(state().settings.trust_lockfile, None);
    scenario.assert_install_gate(super::AllowedRefusal::None);
}

#[test]
fn pruned_exclusions() {
    for shared_lockfile in [true, false] {
        let mut scenario = Scenario::new(Layout { shared_lockfile, ..SHARED });
        for setting in ["minimumReleaseAgeExclude", "trustPolicyExclude"] {
            scenario.set_workspace_setting(&format!("{setting}Prune"), Some("true"));
            scenario.set_workspace_setting(
                setting,
                Some("['@pnpm.e2e/foo@100.0.0', '@pnpm.e2e/fo?', '!example', obsolete]"),
            );
        }
        scenario.install(&[]);
        let yaml = std::fs::read_to_string(scenario.workspace().join("pnpm-workspace.yaml"))
            .expect("read the pruned workspace settings");
        assert_pruned_state(&scenario);
        assert!(!yaml.contains("obsolete"));
        assert!(yaml.contains("@pnpm.e2e/fo?"));
        assert!(yaml.contains("!example"));
        scenario.check_gates();
    }
}

fn assert_pruned_state(scenario: &Scenario) {
    let lockfiles = if scenario.layout.shared_lockfile {
        vec![scenario.lockfile_of("a")]
    } else {
        vec![scenario.lockfile_of("a"), scenario.lockfile_of("b")]
    };
    for lockfile in lockfiles {
        let state = pnpm_workspace_state::load_workspace_state(lockfile.parent().unwrap())
            .unwrap()
            .unwrap();
        for rules in
            [state.settings.minimum_release_age_exclude, state.settings.trust_policy_exclude]
        {
            assert_eq!(rules.unwrap(), ["@pnpm.e2e/foo@100.0.0", "@pnpm.e2e/fo?", "!example"]);
        }
    }
}

#[test]
fn a_dependent_script_can_read_workspace_state_before_exclusions_are_pruned() {
    let scenario = Scenario::new(Layout { shared_lockfile: false, ..SHARED });
    scenario.set_workspace_setting("minimumReleaseAgeExcludePrune", Some("true"));
    scenario.set_workspace_setting("minimumReleaseAgeExclude", Some("[obsolete]"));
    scenario.write_manifest("c", json!({
        "dependencies": {"a": "workspace:*"},
        "scripts": {
            "postinstall": r#"node -e "require('../a/node_modules/.pnpm-workspace-state-v1.json')""#
        }
    }));
    scenario.install(&[]);
    let lockfile = scenario.lockfile_of("a");
    let state =
        pnpm_workspace_state::load_workspace_state(lockfile.parent().unwrap()).unwrap().unwrap();
    assert_eq!(state.settings.minimum_release_age_exclude, None);
}

#[test]
fn frozen_then_filtered_install_after_a_scope_route_moves() {
    let scenario = Scenario::new(SHARED);
    let new_registry =
        super::TestRegistry::start_with_own_storage(scenario.workspace().parent().unwrap());
    scenario.set_workspace_setting("trustLockfile", Some("true"));
    let registry = &scenario.fixture.registry.mock_instance;
    let route = |url: &str| {
        scenario.set_workspace_setting("registries", Some(&format!("{{ '@pnpm.e2e': '{url}' }}")));
    };
    route(registry.url());
    scenario.install(&[]);
    let child = "@pnpm.e2e/dep-of-pkg-with-1-dep";
    let locked = super::read_lockfile(&scenario.lockfile_of("b")).packages
        .unwrap()
        .into_keys()
        .find(|key| key.name.to_string() == child)
        .unwrap();
    new_registry.remove_version(child, &locked.suffix.to_string());
    route(new_registry.url());
    let previous = scenario.snapshot();
    let frozen = scenario.command_at(scenario.workspace(), ["install", "--frozen-lockfile"]);
    super::assert_success(&frozen);
    assert_eq!(scenario.snapshot(), previous);
    scenario.install(&["--filter", "a"]);
    assert!(scenario.exec(&["a"], "error"));
    assert!(scenario.install_takes_fast_path(&["--filter", "a"]));
    let accepted = scenario.exec(&["b"], "error");
    let before = scenario.selection_view(scenario.workspace(), &["b"]);
    scenario.install(&["--config.optimistic-repeat-install=false"]);
    let after = scenario.selection_view(scenario.workspace(), &["b"]);
    assert_ne!(before, after);
    assert!(!accepted);
    scenario.assert_install_gate(super::AllowedRefusal::None);
}
