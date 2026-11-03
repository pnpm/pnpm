use super::{fs, pacquet};
use assert_cmd::assert::OutputAssertExt;
use std::path::{Path, PathBuf};

const CHECKSUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn write_index(cache_dir: &Path, prefix: &str, name: &str, versions: &[&str]) {
    let index_dir = cache_dir.join(format!("v11/cargo-index/crates-io/{prefix}"));
    fs::create_dir_all(&index_dir).unwrap();
    let entry = versions
        .iter()
        .map(|version| {
            format!(
                r#"{{"name":"{name}","vers":"{version}","deps":[],"cksum":"{CHECKSUM}","features":{{}},"yanked":false}}"#,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(index_dir.join(name), entry).unwrap();
}

fn locked_package(name: &str, version: &str) -> String {
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"{CRATES_IO}\"\nchecksum = \"{CHECKSUM}\"\n",
    )
}

/// A Cargo workspace that declares `tower` twice, the old line under the
/// `tower-0-4` alias, and depends on `pre` 2.0.0-rc.1, newer than its
/// newest stable release.
fn setup_workspace(root: &Path) -> PathBuf {
    let workspace = root.join("workspace");
    let cache_dir = root.join("cache");
    let member = workspace.join("crates/app");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(member.join("src/lib.rs"), "").unwrap();
    write_index(&cache_dir, "to/we", "tower", &["0.4.0", "0.4.13", "0.5.0", "0.5.3"]);
    write_index(&cache_dir, "3/p", "pre", &["1.0.0", "2.0.0-rc.1"]);
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
        "[workspace]\nmembers = [\"crates/app\"]\nresolver = \"3\"\n\n[workspace.dependencies]\npre = \"2.0.0-rc.1\"\ntower = \"0.5.0\"\ntower-0-4 = { package = \"tower\", version = \"0.4.0\" }\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npre = { workspace = true }\ntower = { workspace = true }\ntower-0-4 = { workspace = true }\n",
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"pre\", \"tower 0.4.0\", \"tower 0.5.0\"]\n\n{}\n{}\n{}",
            locked_package("pre", "2.0.0-rc.1"),
            locked_package("tower", "0.4.0"),
            locked_package("tower", "0.5.0"),
        ),
    )
    .unwrap();
    workspace
}

#[test]
fn update_latest_keeps_a_renamed_alias_and_a_prerelease_on_their_lines() {
    let root = tempfile::tempdir().unwrap();
    let workspace = setup_workspace(root.path());

    pacquet(&workspace, ["update", "--latest", "--offline", "--lockfile-only"]).assert().success();

    let cargo_toml = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    eprintln!("CARGO.TOML:\n{cargo_toml}\n");
    assert!(cargo_toml.contains(r#"tower = "0.5.3""#));
    assert!(cargo_toml.contains(r#"tower-0-4 = { package = "tower", version = "0.4.13" }"#));
    assert!(cargo_toml.contains(r#"pre = "2.0.0-rc.1""#));
    let cargo_lock = fs::read_to_string(workspace.join("Cargo.lock")).unwrap();
    eprintln!("CARGO.LOCK:\n{cargo_lock}\n");
    assert!(cargo_lock.contains("name = \"tower\"\nversion = \"0.4.13\""));
    assert!(cargo_lock.contains("name = \"tower\"\nversion = \"0.5.3\""));
    assert!(cargo_lock.contains("name = \"pre\"\nversion = \"2.0.0-rc.1\""));
}
