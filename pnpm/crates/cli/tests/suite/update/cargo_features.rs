use super::{fs, pacquet};
use pnpm_testing_utils::diagnostics::assert_diagnostic_contains;
use std::path::{Path, PathBuf};

const CHECKSUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A Cargo workspace whose member enables `asm` on `foo` through a
/// target-specific table. `foo` 2.0.0 drops `asm`; `bar` 2.0.0 is an
/// ordinary new major.
fn setup_workspace(root: &Path) -> PathBuf {
    let workspace = root.join("workspace");
    let cache_dir = root.join("cache");
    let member = workspace.join("crates/app");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(member.join("src/lib.rs"), "").unwrap();
    for (prefix, name, old_features, new_features) in
        [("3/f", "foo", r#"{"asm":[]}"#, "{}"), ("3/b", "bar", "{}", "{}")]
    {
        let index_dir = cache_dir.join(format!("v11/cargo-index/crates-io/{prefix}"));
        fs::create_dir_all(&index_dir).unwrap();
        let entry = [("1.0.0", old_features), ("2.0.0", new_features)]
            .iter()
            .map(|(version, features)| {
                format!(
                    r#"{{"name":"{name}","vers":"{version}","deps":[],"cksum":"{CHECKSUM}","features":{features},"yanked":false}}"#,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(index_dir.join(name), entry).unwrap();
    }
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "cargo:\n  enabled: true\nstoreDir: '{}'\ncacheDir: '{}'\n",
            root.join("store").display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nbar = \"1.0.0\"\nfoo = \"1.0.0\"\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nbar = { workspace = true }\nfoo = { workspace = true }\n\n[target.'cfg(unix)'.dependencies]\nfoo = { workspace = true, features = [\"asm\"] }\n",
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"bar\", \"foo\"]\n\n[[package]]\nname = \"bar\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{CHECKSUM}\"\n\n[[package]]\nname = \"foo\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{CHECKSUM}\"\n",
        ),
    )
    .unwrap();
    workspace
}

#[test]
fn update_latest_keeps_a_crate_whose_new_version_lacks_an_enabled_feature() {
    let root = tempfile::tempdir().unwrap();
    let workspace = setup_workspace(root.path());

    let output = pacquet(&workspace, ["update", "--latest", "--offline", "--lockfile-only"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_diagnostic_contains(
        &format!("{stdout}{stderr}"),
        "Not updating crate foo 2.0.0: it does not have the features this workspace enables on it: `asm`",
    );
    let cargo_toml = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains(r#"foo = "1.0.0""#), "{cargo_toml}");
    assert!(cargo_toml.contains(r#"bar = "2.0.0""#), "{cargo_toml}");
    let cargo_lock = fs::read_to_string(workspace.join("Cargo.lock")).unwrap();
    assert!(cargo_lock.contains("name = \"foo\"\nversion = \"1.0.0\""), "{cargo_lock}");
    assert!(cargo_lock.contains("name = \"bar\"\nversion = \"2.0.0\""), "{cargo_lock}");
}

#[test]
fn update_latest_fails_for_a_named_crate_whose_new_version_lacks_an_enabled_feature() {
    let root = tempfile::tempdir().unwrap();
    let workspace = setup_workspace(root.path());
    let original = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();

    let output =
        pacquet(&workspace, ["update", "--latest", "crate:foo", "--offline", "--lockfile-only"])
            .output()
            .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert_diagnostic_contains(
        &stderr,
        "cannot update crate foo 2.0.0: it does not have the features this workspace enables on it: `asm`",
    );
    assert_eq!(fs::read_to_string(workspace.join("Cargo.toml")).unwrap(), original);
}
