use super::State;
use pacquet_config::Config;

#[test]
fn workspace_state_anchors_lockfile_at_workspace_root() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let workspace_root = temp.path().to_path_buf();
    let project_dir = workspace_root.join("packages/app");
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    let manifest_path = project_dir.join("package.json");
    std::fs::write(&manifest_path, r#"{"name":"app"}"#).expect("write project manifest");

    let config =
        Config::leak(Config { workspace_dir: Some(workspace_root.clone()), ..Config::default() });
    let state = State::init(manifest_path, config, false).expect("initialize state");

    assert_eq!(state.lockfile_dir(), workspace_root);
    assert_eq!(state.lockfile_path(), workspace_root.join(pacquet_lockfile::Lockfile::FILE_NAME));
    assert_eq!(state.active_importer_id(), "packages/app");
}

#[test]
fn workspace_state_anchors_per_project_lockfile_at_project_root() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let workspace_root = temp.path().to_path_buf();
    let project_dir = workspace_root.join("packages/app");
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    let manifest_path = project_dir.join("package.json");
    std::fs::write(&manifest_path, r#"{"name":"app"}"#).expect("write project manifest");

    let config = Config::leak(Config {
        workspace_dir: Some(workspace_root),
        shared_workspace_lockfile: false,
        ..Config::default()
    });
    let state = State::init(manifest_path, config, false).expect("initialize state");

    assert_eq!(state.lockfile_dir(), project_dir);
    assert_eq!(state.lockfile_path(), project_dir.join(pacquet_lockfile::Lockfile::FILE_NAME));
    assert_eq!(state.active_importer_id(), ".");
}
