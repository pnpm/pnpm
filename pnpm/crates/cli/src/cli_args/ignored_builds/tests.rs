use super::render_ignored_builds;
use pnpm_config::Config;
use std::{
    fs,
    path::Path,
};
use tempfile::tempdir;

const HINTS: &str = "\nhint: To allow the execution of build scripts for a package, add its name to \"permissions\" with \"build: true\", then run \"pnpm rebuild\".\nhint: For example:\nhint: permissions:\nhint:   esbuild:\nhint:     build: true\nhint: If you don't want to build a package, set \"build\" to \"false\" instead.";

/// Build a `Config` whose `modules_dir` is a `node_modules` under `dir`
/// and whose `allow_builds` records each `disallowed` name as `false`.
/// When `ignored_builds` is `Some`, a `.modules.yaml` recording those
/// depPaths is written; `None` leaves `node_modules` absent (the
/// "cannot identify" case).
fn config_with(dir: &Path, ignored_builds: Option<&[&str]>, disallowed: &[&str]) -> Config {
    let modules_dir = dir.join("node_modules");
    if let Some(ignored) = ignored_builds {
        fs::create_dir_all(&modules_dir).expect("create node_modules");
        let manifest = serde_json::json!({
            "layoutVersion": 5,
            "packageManager": "pacquet@test",
            "ignoredBuilds": ignored,
            "storeDir": "/store",
            "virtualStoreDir": ".pnpm",
        });
        fs::write(modules_dir.join(".modules.yaml"), manifest.to_string())
            .expect("write .modules.yaml");
    }
    let mut config = Config::new();
    config.modules_dir = modules_dir;
    for name in disallowed {
        config.allow_builds.insert((*name).to_string(), false);
    }
    config
}

// Ports pnpm's `ignoredBuilds lists automatically ignored dependencies`.
#[test]
fn lists_automatically_ignored_dependencies() {
    let dir = tempdir().unwrap();
    let config = config_with(dir.path(), Some(&["foo@1.0.0"]), &[]);
    let output = render_ignored_builds(&config).unwrap();
    assert_eq!(
        output,
        format!("Automatically ignored builds during installation:\n  foo{HINTS}\n"),
    );
}

// Ports pnpm's `ignoredBuilds lists explicitly ignored dependencies`.
#[test]
fn lists_explicitly_ignored_dependencies() {
    let dir = tempdir().unwrap();
    let config = config_with(dir.path(), Some(&[]), &["bar"]);
    let output = render_ignored_builds(&config).unwrap();
    assert_eq!(
        output,
        "Automatically ignored builds during installation:\n  None\n\nExplicitly ignored package builds:\n  bar\n",
    );
}

// Ports pnpm's `ignoredBuilds lists both automatically and explicitly
// ignored dependencies`.
#[test]
fn lists_both_automatically_and_explicitly_ignored() {
    let dir = tempdir().unwrap();
    let config = config_with(dir.path(), Some(&["foo@1.0.0", "bar@1.0.0"]), &["qar", "zoo"]);
    let output = render_ignored_builds(&config).unwrap();
    assert_eq!(
        output,
        format!(
            "Automatically ignored builds during installation:\n  foo\n  bar{HINTS}\n\nExplicitly ignored package builds:\n  qar\n  zoo\n",
        ),
    );
}

// Ports pnpm's `ignoredBuilds prints an info message when there is no
// node_modules`.
#[test]
fn reports_cannot_identify_when_no_node_modules() {
    let dir = tempdir().unwrap();
    let config = config_with(dir.path(), None, &["qar", "zoo"]);
    let output = render_ignored_builds(&config).unwrap();
    assert_eq!(
        output,
        "Automatically ignored builds during installation:\n  Cannot identify as no node_modules found\n\nExplicitly ignored package builds:\n  qar\n  zoo\n",
    );
}
