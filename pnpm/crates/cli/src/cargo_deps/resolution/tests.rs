use super::{resolution_command, resolution_settings};
use crate::cargo_deps::build_std;
use std::fs;
use tempfile::TempDir;

#[test]
fn resolution_only_copies_audited_settings_from_checkout_configuration() {
    let parent = TempDir::new().unwrap();
    let child = parent.path().join("child");
    for root in [parent.path(), &child] {
        fs::create_dir_all(root.join(".cargo")).unwrap();
    }
    fs::write(
        parent.path().join(".cargo/config"),
        r#"
[unstable]
bindeps = false
[resolver]
incompatible-rust-versions = "fallback"
"#,
    )
    .unwrap();
    fs::write(
        child.join(".cargo/config.toml"),
        r#"
[unstable]
bindeps = true
[env]
RUSTC = { value = "checkout-command", force = true }
[registry]
global-credential-providers = ["checkout-command"]
[net]
git-fetch-with-cli = true
[source.crates-io]
replace-with = "checkout-source"
"#,
    )
    .unwrap();

    let settings = resolution_settings(&child).unwrap();
    let command = resolution_command(&child, true).unwrap();

    assert_eq!(settings.len(), 2);
    assert_eq!(settings["unstable.bindeps"], toml::Value::Boolean(true));
    assert_eq!(settings["resolver.incompatible-rust-versions"].as_str(), Some("fallback"));
    assert_eq!(command.get_current_dir(), Some(build_std::sysroot(&child).unwrap().as_path()));
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    eprintln!("Only audited checkout settings may reach Cargo: {args:?}");
    assert!(
        args.iter()
            .all(|arg| !arg.contains("checkout-command") && !arg.contains("git-fetch-with-cli")),
    );
    assert_eq!(args.last().map(String::as_str), Some("--offline"));
}

#[test]
fn malformed_resolution_settings_are_rejected() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join(".cargo")).unwrap();
    for contents in [
        r#"[unstable]
bindeps = "command"
"#,
        r#"[resolver]
incompatible-rust-versions = "command"
"#,
    ] {
        fs::write(root.path().join(".cargo/config.toml"), contents).unwrap();
        let error = resolution_settings(root.path()).unwrap_err();
        eprintln!("Invalid settings must fail: {error:?}");
        assert!(error.to_string().contains("invalid Cargo resolution setting"));
    }
}

#[test]
fn root_source_overrides_are_detected_and_git_transports_are_validated() {
    let root = TempDir::new().unwrap();
    for (manifest, expected) in [
        ("[workspace]\n", false),
        ("[patch.crates-io]\ndemo = { path = \"dep\" }\n", true),
        (
            "[patch.\"https://example.test/index\"]\ndemo = { git = \"https://example.test/demo\", rev = \"abc\" }\n",
            true,
        ),
        ("[replace]\n\"demo:1.0.0\" = { path = \"dep\" }\n", true),
    ] {
        fs::write(root.path().join("Cargo.toml"), manifest).unwrap();
        assert_eq!(super::has_source_overrides(root.path()).unwrap(), expected);
    }
    for manifest in [
        "[patch.crates-io]\ndemo = { git = \"ext://checkout-command\" }\n",
        "[replace]\n\"demo:1.0.0\" = { git = \"ext://checkout-command\" }\n",
    ] {
        fs::write(root.path().join("Cargo.toml"), manifest).unwrap();
        let error = super::has_source_overrides(root.path()).unwrap_err();
        eprintln!("Unsupported override transports must fail before Cargo runs: {error:?}");
        assert!(error.to_string().contains("transport"));
    }
}
