use super::ProvisionedTool;
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;

#[test]
fn commands_are_read_from_the_installed_manifest() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"@yarnpkg/cli-dist","bin":{"yarn":"./bin/yarn.js","yarnpkg":"./bin/yarn.js"}}"#,
    )
    .unwrap();

    let tool = ProvisionedTool::from_dir("yarn".to_string(), "4.9.2".to_string(), dir.path(), true)
        .expect("read the tool");

    assert_eq!(tool.command("yarnpkg"), Some(dir.path().join("bin/yarn.js").as_path()));
    assert!(tool.runs_on_node);
}

/// A single-executable Yarn publishes no `yarnpkg`, and a build that calls
/// it still gets the one Yarn.
#[test]
fn a_missing_alias_falls_back_to_the_tool_command() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"yarn","bin":{"yarn":"yarn"}}"#).unwrap();

    let tool =
        ProvisionedTool::from_dir("yarn".to_string(), "6.0.0".to_string(), dir.path(), false)
            .expect("read the tool");

    assert_eq!(tool.command("yarnpkg"), Some(dir.path().join("yarn").as_path()));
}
