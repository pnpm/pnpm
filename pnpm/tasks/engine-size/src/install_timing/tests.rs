use super::{benchmark_env, prepare_project};
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::Path};

#[test]
fn benchmark_env_removes_inherited_settings_but_keeps_credentials() {
    let project = Path::new("/bench/project");
    let inherited = [
        "PNPM_CONFIG_MODULES_DIR",
        "pnpm_config_lockfile_dir",
        "npm_config_store_dir",
        "PNPM_CONFIG_REGISTRY",
        "npm_config_//registry.example/:_authToken",
        "NPM_CONFIG__AUTH",
        "NPM_CONFIG_USERCONFIG",
        "PNPM_CONFIG_NPMRC_AUTH_FILE",
        "PNPM_CONFIG_HTTPS_PROXY",
        "npm_config_strict-ssl",
        "PATH",
    ]
    .map(OsString::from);
    let env = benchmark_env(project, "https://registry.example/", inherited);
    let expected: Vec<(OsString, Option<OsString>)> = vec![
        ("PNPM_CONFIG_MODULES_DIR".into(), None),
        ("pnpm_config_lockfile_dir".into(), None),
        ("npm_config_store_dir".into(), None),
        ("PNPM_CONFIG_REGISTRY".into(), None),
        ("PNPM_CONFIG_REGISTRY".into(), Some("https://registry.example/".into())),
        ("PNPM_CONFIG_STORE_DIR".into(), Some(project.join("store-dir").into())),
        ("PNPM_CONFIG_CACHE_DIR".into(), Some(project.join("cache-dir").into())),
        ("PNPM_CONFIG_IGNORE_SCRIPTS".into(), Some("true".into())),
        ("PNPM_CONFIG_MODULES_DIR".into(), Some(project.join("node_modules").into())),
        ("PNPM_CONFIG_VIRTUAL_STORE_DIR".into(), Some(project.join("node_modules/.pnpm").into())),
    ];
    assert_eq!(env, expected);
}

#[test]
fn prepare_project_skips_install_output_but_keeps_nested_inputs() {
    let fixture = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    for file in [
        "package.json",
        "patches/a.patch",
        "packages/store-dir/package.json",
        "store-dir/index",
        "cache-dir/meta",
        "node_modules/a/package.json",
        "packages/b/node_modules/c/package.json",
    ] {
        let path = fixture.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}").unwrap();
    }
    prepare_project(fixture.path(), project.path()).unwrap();
    let mut copied: Vec<String> = walkdir::WalkDir::new(project.path())
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            let relative = entry
                .path()
                .strip_prefix(project.path())
                .unwrap();
            relative.to_string_lossy().replace('\\', "/")
        })
        .collect();
    copied.sort();
    assert_eq!(copied, ["package.json", "packages/store-dir/package.json", "patches/a.patch"]);
}

#[cfg(unix)]
#[test]
fn prepare_project_recreates_directory_symlinks() {
    let fixture = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("real")).unwrap();
    std::os::unix::fs::symlink("real", fixture.path().join("link")).unwrap();
    prepare_project(fixture.path(), project.path()).unwrap();
    assert_eq!(std::fs::read_link(project.path().join("link")).unwrap(), Path::new("real"));
}

#[cfg(unix)]
#[test]
fn prepare_project_points_links_that_leave_the_fixture_at_their_absolute_target() {
    let root = tempfile::tempdir().unwrap();
    let fixture = root.path().join("fixture");
    std::fs::create_dir(&fixture).unwrap();
    std::fs::write(root.path().join("manifest.json"), r#"{"name":"shared"}"#).unwrap();
    std::os::unix::fs::symlink("../manifest.json", fixture.join("package.json")).unwrap();
    let project = tempfile::tempdir().unwrap();
    prepare_project(&fixture, project.path()).unwrap();
    let copied = std::fs::read_to_string(project.path().join("package.json")).unwrap();
    assert_eq!(copied, r#"{"name":"shared"}"#);
}

#[cfg(unix)]
#[test]
fn prepare_project_keeps_dangling_links() {
    let fixture = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("missing", fixture.path().join("dangling")).unwrap();
    prepare_project(fixture.path(), project.path()).unwrap();
    assert_eq!(std::fs::read_link(project.path().join("dangling")).unwrap(), Path::new("missing"));
}
