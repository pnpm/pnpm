#[cfg(unix)]
use super::configs_in_scope;
use super::{
    resolution_command,
    resolution_settings,
};
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

    let settings = resolution_settings(&child, None).unwrap();
    let command = resolution_command(&child, None, true).unwrap();

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

/// A developer's own `~/.cargo/config.toml` is commonly a symlink into a
/// dotfiles repository, and it is an ancestor of every workspace under the
/// home directory.
#[cfg(unix)]
#[test]
fn a_symlinked_configuration_in_an_ancestor_is_read() {
    let parent = TempDir::new().unwrap();
    let child = parent.path().join("child");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir_all(parent.path().join(".cargo")).unwrap();
    let elsewhere = parent.path().join("dotfiles-config.toml");
    fs::write(&elsewhere, "[resolver]\nincompatible-rust-versions = \"fallback\"\n").unwrap();
    std::os::unix::fs::symlink(&elsewhere, parent.path().join(".cargo/config.toml")).unwrap();

    let settings = resolution_settings(&child, Some(&child)).unwrap();

    assert_eq!(settings["resolver.incompatible-rust-versions"].as_str(), Some("fallback"));
}

/// The workspace's own file is one pnpm writes into the checkout, so the
/// containment that guards the write guards this read too.
#[cfg(unix)]
#[test]
fn a_symlinked_configuration_at_the_workspace_root_is_refused() {
    let root = TempDir::new().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join(".cargo")).unwrap();
    let outside = root.path().join("outside.toml");
    fs::write(&outside, "[unstable]\nbindeps = true\n").unwrap();
    std::os::unix::fs::symlink(&outside, workspace.join(".cargo/config.toml")).unwrap();

    let error = resolution_settings(&workspace, None).unwrap_err();

    assert!(error.to_string().contains("config.toml"), "unexpected error: {error}");
}

/// A nearer configuration can answer on its own, and a file the answer does
/// not need must not be able to fail the run.
#[cfg(unix)]
#[test]
fn a_farther_unreadable_configuration_is_left_unread() {
    let root = TempDir::new().unwrap();
    let grandparent = root.path().join("grandparent");
    let child = grandparent.join("parent/child");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir_all(grandparent.join("parent/.cargo")).unwrap();
    fs::create_dir_all(grandparent.join(".cargo")).unwrap();
    fs::write(grandparent.join("parent/.cargo/config.toml"), "[unstable]\nbindeps = true\n")
        .unwrap();
    std::os::unix::fs::symlink(
        grandparent.join("nothing-here.toml"),
        grandparent.join(".cargo/config.toml"),
    )
    .unwrap();

    let nearest = configs_in_scope(&child, Some(&child))
        .next()
        .expect("a configuration in scope")
        .expect("read the nearest configuration");

    assert!(nearest.contains("bindeps"), "unexpected configuration: {nearest}");
}

/// Every setting the resolution command carries is already answered by the
/// nearer file, so the walk stops before the unreadable one above it.
#[cfg(unix)]
#[test]
fn settings_resolved_by_a_nearer_configuration_stop_the_walk() {
    let root = TempDir::new().unwrap();
    let grandparent = root.path().join("grandparent");
    let child = grandparent.join("parent/child");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir_all(grandparent.join("parent/.cargo")).unwrap();
    fs::create_dir_all(grandparent.join(".cargo")).unwrap();
    fs::write(
        grandparent.join("parent/.cargo/config.toml"),
        "[unstable]\nbindeps = true\n[resolver]\nincompatible-rust-versions = \"fallback\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        grandparent.join("nothing-here.toml"),
        grandparent.join(".cargo/config.toml"),
    )
    .unwrap();

    let settings = resolution_settings(&child, Some(&child)).expect("resolve the settings");

    assert_eq!(settings["unstable.bindeps"], toml::Value::Boolean(true));
    assert_eq!(settings["resolver.incompatible-rust-versions"].as_str(), Some("fallback"));
}

/// A Cargo workspace may sit inside a larger checkout, whose own
/// configuration file is repository content however far above it lives.
#[cfg(unix)]
#[test]
fn a_symlinked_configuration_inside_the_checkout_is_refused() {
    let root = TempDir::new().unwrap();
    let checkout = root.path().join("checkout");
    let workspace = checkout.join("crates/app");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(checkout.join(".cargo")).unwrap();
    let outside = root.path().join("outside.toml");
    fs::write(&outside, "[unstable]\nbindeps = true\n").unwrap();
    std::os::unix::fs::symlink(&outside, checkout.join(".cargo/config.toml")).unwrap();

    let error = resolution_settings(&workspace, Some(&checkout)).unwrap_err();

    assert!(error.to_string().contains("config.toml"), "unexpected error: {error}");
}

/// A boundary pnpm cannot resolve says nothing about what a checkout
/// controls, so every file in scope is treated as its content.
#[cfg(unix)]
#[test]
fn an_unresolved_checkout_refuses_a_symlinked_configuration() {
    let parent = TempDir::new().unwrap();
    let child = parent.path().join("child");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir_all(parent.path().join(".cargo")).unwrap();
    let elsewhere = parent.path().join("dotfiles-config.toml");
    fs::write(&elsewhere, "[resolver]\nincompatible-rust-versions = \"fallback\"\n").unwrap();
    std::os::unix::fs::symlink(&elsewhere, parent.path().join(".cargo/config.toml")).unwrap();

    let error = resolution_settings(&child, None).unwrap_err();

    assert!(error.to_string().contains("config.toml"), "unexpected error: {error}");
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
        let error = resolution_settings(root.path(), None).unwrap_err();
        eprintln!("Invalid settings must fail: {error:?}");
        assert!(error.to_string().contains("invalid Cargo resolution setting"));
    }
}

#[test]
fn root_source_overrides_are_detected_and_git_transports_are_validated() {
    let root = TempDir::new().unwrap();
    for (manifest, expected) in [
        ("[workspace]\n", false),
        ("[patch]\n", false),
        ("[patch.crates-io]\n", false),
        ("[patch.crates-io]\n[patch.\"https://example.test/index\"]\n", false),
        ("[replace]\n", false),
        ("[patch.crates-io]\n[replace]\n", false),
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
