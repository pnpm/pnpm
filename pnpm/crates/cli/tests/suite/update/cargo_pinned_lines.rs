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

#[test]
fn targeted_update_keeps_an_unrelated_transitive_crate_locked() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let cache_dir = root.path().join("cache");
    fs::create_dir_all(workspace.join("src")).unwrap();
    fs::write(workspace.join("src/lib.rs"), "").unwrap();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "cargo:\n  enabled: true\nstoreDir: '{}'\ncacheDir: '{}'\n",
            root.path().join("store").display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfoo = \"1\"\n",
    )
    .unwrap();
    let foo_index_dir = cache_dir.join("v11/cargo-index/crates-io/3/f");
    let bar_index_dir = cache_dir.join("v11/cargo-index/crates-io/3/b");
    fs::create_dir_all(&foo_index_dir).unwrap();
    fs::create_dir_all(&bar_index_dir).unwrap();
    fs::write(
        foo_index_dir.join("foo"),
        concat!(
            r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#,
            "\n",
            r#"{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#,
        ),
    )
    .unwrap();
    fs::write(
        bar_index_dir.join("bar"),
        concat!(
            r#"{"name":"bar","vers":"1.0.0","deps":[],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{},"yanked":false}"#,
            "\n",
            r#"{"name":"bar","vers":"1.1.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":false}"#,
        ),
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"foo\"]\n\n{}\n{}",
            locked_package("foo", "1.0.0"),
            locked_package("bar", "1.0.0"),
        ),
    )
    .unwrap();

    pacquet(&workspace, ["update", "--latest", "crate:foo", "--offline", "--lockfile-only"])
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("Cargo.lock")).unwrap();
    assert!(lockfile.contains("name = \"foo\"\nversion = \"1.1.0\""), "{lockfile}");
    assert!(lockfile.contains("name = \"bar\"\nversion = \"1.0.0\""), "{lockfile}");
}

#[test]
fn update_latest_keeps_the_requirement_operator_over_save_prefix() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let cache_dir = root.path().join("cache");
    fs::create_dir_all(workspace.join("src")).unwrap();
    fs::write(workspace.join("src/lib.rs"), "").unwrap();
    write_index(&cache_dir, "ra/ng", "range", &["1.0.0", "1.1.0"]);
    write_index(&cache_dir, "ti/ld", "tilde", &["1.0.0", "1.1.0"]);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "cargo:\n  enabled: true\nsavePrefix: ''\nstoreDir: '{}'\ncacheDir: '{}'\n",
            root.path().join("store").display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nrange = \">=1.0.0, <2\"\ntilde = \"~1.0.0\"\n",
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"range\", \"tilde\"]\n\n{}\n{}",
            locked_package("range", "1.0.0"),
            locked_package("tilde", "1.0.0"),
        ),
    )
    .unwrap();

    pacquet(&workspace, ["update", "--latest", "--offline", "--lockfile-only"]).assert().success();

    let cargo_toml = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    eprintln!("CARGO.TOML:\n{cargo_toml}\n");
    assert!(cargo_toml.contains(r#"range = ">=1.1.0, <2""#));
    assert!(cargo_toml.contains(r#"tilde = "~1.1.0""#));
}

#[test]
fn update_latest_resolves_member_package_rename_inherited_from_workspace() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let cache_dir = root.path().join("cache");
    let member = workspace.join("crates/app");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(member.join("src/lib.rs"), "").unwrap();
    write_index(&cache_dir, "it/oa", "itoa", &["1.0.0", "1.0.18"]);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "cargo:\n  enabled: true\nstoreDir: '{}'\ncacheDir: '{}'\n",
            root.path().join("store").display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nmy_itoa = { package = \"itoa\", version = \"1.0.0\" }\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nmy_itoa = { package = \"itoa\", workspace = true }\n",
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"itoa\"]\n\n{}",
            locked_package("itoa", "1.0.0"),
        ),
    )
    .unwrap();

    pacquet(&workspace, ["update", "--latest", "--offline", "--lockfile-only"]).assert().success();

    let root_toml = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    assert!(
        root_toml.contains(r#"my_itoa = { package = "itoa", version = "1.0.18" }"#),
        "ROOT CARGO.TOML:\n{root_toml}",
    );
    let member_toml = fs::read_to_string(member.join("Cargo.toml")).unwrap();
    assert!(
        member_toml.contains(r#"my_itoa = { package = "itoa", workspace = true }"#),
        "MEMBER CARGO.TOML:\n{member_toml}",
    );
    let cargo_lock = fs::read_to_string(workspace.join("Cargo.lock")).unwrap();
    assert!(cargo_lock.contains("name = \"itoa\"\nversion = \"1.0.18\""), "{cargo_lock}");
}
