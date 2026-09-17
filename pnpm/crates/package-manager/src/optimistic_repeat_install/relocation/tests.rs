use super::relocated_state;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{ProjectEntry, WorkspaceState};
use std::path::{Path, PathBuf};

fn state_recording(dirs: &[PathBuf], pnpmfiles: Vec<String>) -> WorkspaceState {
    WorkspaceState {
        last_validated_timestamp: 1_700_000_000_000,
        projects: dirs
            .iter()
            .map(|dir| (path_string(dir), ProjectEntry::default()))
            .collect(),
        pnpmfiles,
        ..WorkspaceState::default()
    }
}

/// [`relocated_state`] over the nameless projects [`state_recording`] records.
fn relocated_onto(
    state: &WorkspaceState,
    workspace_root: &Path,
    dirs: &[PathBuf],
) -> Option<WorkspaceState> {
    let manifests: Vec<PackageManifest> = dirs
        .iter()
        .map(|dir| PackageManifest::from_value(dir.join("package.json"), serde_json::json!({})))
        .collect();
    let project_manifests: Vec<(PathBuf, &PackageManifest)> = dirs
        .iter()
        .cloned()
        .zip(&manifests)
        .collect();
    relocated_state(state, workspace_root, &project_manifests)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn rebases_the_projects_and_the_pnpmfiles_inside_the_recorded_root() {
    let old_root = Path::new("/old/ws");
    let new_root = Path::new("/new/deeper/ws");
    let global_pnpmfile = path_string(Path::new("/global/pnpmfile.cjs"));
    let state = state_recording(
        &[old_root.to_path_buf(), old_root.join("packages").join("a")],
        vec![path_string(&old_root.join(".pnpmfile.cjs")), global_pnpmfile.clone()],
    );

    let current = [new_root.to_path_buf(), new_root.join("packages").join("a")];
    let relocated = relocated_onto(&state, new_root, &current)
        .expect("the state records the same projects under another root");

    dbg!(&relocated);
    let keys: Vec<&String> = relocated.projects.keys().collect();
    assert_eq!(keys, [&path_string(&current[0]), &path_string(&current[1])]);
    assert_eq!(
        relocated.pnpmfiles,
        [path_string(&new_root.join(".pnpmfile.cjs")), global_pnpmfile],
    );
}

/// Nothing is re-keyed but a whole tree proven to have moved under one
/// other root.
#[test]
fn refuses_to_rebase_anything_else() {
    let root = Path::new("/ws");
    let in_place = [root.to_path_buf(), root.join("a")];
    let state = state_recording(&in_place, Vec::new());
    let new_root = Path::new("/new/ws");
    let under_a_new_root = [new_root.to_path_buf(), new_root.join("packages").join("a")];
    let cases = [
        ("recorded in place", root, &in_place[..]),
        ("one project renamed in place", root, &[root.to_path_buf(), root.join("b")][..]),
        ("a rebase reproducing another layout", new_root, &under_a_new_root[..]),
    ];

    for (label, workspace_root, dirs) in cases {
        assert_eq!(relocated_onto(&state, workspace_root, dirs), None, "{label}");
    }
}
