use super::{GitPackage, GitSource, VendorSourceOptions, vendor_source};
use crate::cargo_deps::git::manifest::{Manifest, vendored_package};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::StoreDir;
use pnpm_testing_utils::git_repo::GitRepoFixture;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

fn manifest(text: &str) -> Manifest {
    Manifest { text: text.to_string(), document: toml::from_str(text).expect("parse manifest") }
}

fn source_id(source: &str) -> cargo_lock::SourceId {
    source.parse().expect("parse Cargo source")
}

/// Commit `files` into a fresh repository, answering the URL and commit
/// a Cargo git source names it by.
fn commit_repository(root: &Path, files: &[(&str, &str)]) -> (String, String) {
    let repository = GitRepoFixture::init(root, "repo");
    for (path, contents) in files {
        repository.write_file(path, contents);
    }
    let commit = repository.commit("init");
    (repository.file_url(), commit)
}

#[test]
fn a_manifest_that_inherits_nothing_is_vendored_verbatim() {
    let text = "# a comment\n[package]\nname = \"demo\"\nversion = \"1.0.0\"\n";

    let vendored = vendored_package(&manifest(text), None).unwrap();

    assert_eq!(vendored.version, "1.0.0");
    assert_eq!(vendored.manifest, text);
}

#[test]
fn workspace_inheritance_is_resolved_into_the_vendored_manifest() {
    let workspace = manifest(
        r#"
[workspace]
members = ["member"]

[workspace.package]
version = "0.3.0"
edition = "2021"

[workspace.dependencies]
libc = { version = "0.2", default-features = false, features = ["extra"] }
serde = "1"

[workspace.lints.rust]
unsafe_code = "forbid"
"#,
    );
    let member = manifest(
        r#"
[package]
name = "member"
version.workspace = true
edition.workspace = true

[dependencies]
libc = { workspace = true, optional = true, features = ["std"] }

[build-dependencies]
serde.workspace = true

[lints]
workspace = true
"#,
    );

    let vendored = vendored_package(&member, Some(&workspace.document)).unwrap();

    assert_eq!(vendored.version, "0.3.0");
    let document: toml::Table = toml::from_str(&vendored.manifest).expect("valid TOML");
    dbg!(&document);
    assert_eq!(document["package"]["edition"].as_str(), Some("2021"));
    assert_eq!(document["dependencies"]["libc"]["version"].as_str(), Some("0.2"));
    assert_eq!(document["dependencies"]["libc"]["optional"].as_bool(), Some(true));
    assert_eq!(document["dependencies"]["libc"]["default-features"].as_bool(), Some(false));
    assert_eq!(
        document["dependencies"]["libc"]["features"].as_array().map(Vec::as_slice),
        Some(["extra".into(), "std".into()].as_slice()),
    );
    assert_eq!(document["build-dependencies"]["serde"]["version"].as_str(), Some("1"));
    assert_eq!(document["lints"]["rust"]["unsafe_code"].as_str(), Some("forbid"));
}

#[test]
fn the_workspace_table_is_left_out_of_a_vendored_root_package() {
    let root = manifest(
        r#"
[package]
name = "root"
version = "1.0.0"

[workspace]
members = ["member"]
"#,
    );

    let vendored = vendored_package(&root, Some(&root.document)).unwrap();

    assert!(!vendored.manifest.contains("[workspace]"), "{}", vendored.manifest);
}

#[test]
fn an_uninheritable_field_is_reported() {
    let member = manifest("[package]\nname = \"member\"\nversion.workspace = true\n");

    let error = vendored_package(&member, Some(&manifest("[workspace]\n").document))
        .unwrap_err()
        .to_string();

    assert!(error.contains("no `package.version` to inherit"), "{error}");
}

#[test]
fn a_git_source_is_replaced_by_the_vendored_directory() {
    let source = GitSource::from_source_id(&source_id(
        "git+https://example.test/repo?rev=1ae976a#1ae976a0023b4dec80b1a5411ccee8343f91320e",
    ))
    .unwrap();

    assert_eq!(
        source.config_block(),
        concat!(
            "[source.\"git+https://example.test/repo?rev=1ae976a\"]\n",
            "git = \"https://example.test/repo\"\n",
            "rev = \"1ae976a\"\n",
            "replace-with = \"pnpm-git\"\n",
        ),
    );
}

#[test]
fn a_branch_source_keeps_the_branch_it_was_locked_from() {
    let source = GitSource::from_source_id(&source_id(
        "git+https://example.test/repo?branch=next#1ae976a0023b4dec80b1a5411ccee8343f91320e",
    ))
    .unwrap();

    assert!(source.config_block().contains("branch = \"next\"\n"), "{}", source.config_block());
}

#[test]
fn a_default_branch_source_names_no_reference() {
    let source = GitSource::from_source_id(&source_id(
        "git+https://example.test/repo#1ae976a0023b4dec80b1a5411ccee8343f91320e",
    ))
    .unwrap();

    assert_eq!(
        source.config_block(),
        concat!(
            "[source.\"git+https://example.test/repo\"]\n",
            "git = \"https://example.test/repo\"\n",
            "replace-with = \"pnpm-git\"\n",
        ),
    );
}

#[test]
fn a_source_that_names_a_transport_pnpm_does_not_fetch_over_is_refused() {
    let error = GitSource::from_source_id(&source_id(
        "git+ext::sh#1ae976a0023b4dec80b1a5411ccee8343f91320e",
    ))
    .unwrap_err()
    .to_string();

    assert!(error.contains("does not fetch a git dependency over"), "{error}");
}

#[test]
fn a_source_without_a_locked_commit_is_refused() {
    let error = GitSource::from_source_id(&source_id("git+https://example.test/repo?branch=next"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("pins no commit"), "{error}");
}

fn vendor_from(
    repository: &str,
    commit: &str,
    store_dir: &StoreDir,
    packages: &[(&str, &str)],
) -> Vec<(String, PathBuf)> {
    vendor_from_offline(repository, commit, store_dir, packages, false)
}

fn vendor_from_offline(
    repository: &str,
    commit: &str,
    store_dir: &StoreDir,
    packages: &[(&str, &str)],
    offline: bool,
) -> Vec<(String, PathBuf)> {
    let source = Arc::new(
        GitSource::from_source_id(&source_id(&format!("git+{repository}#{commit}"))).unwrap(),
    );
    let packages = packages
        .iter()
        .map(|(name, version)| GitPackage {
            name: (*name).to_string(),
            version: (*version).to_string(),
            source: Arc::clone(&source),
        })
        .collect::<Vec<_>>();
    vendor_source::<SilentReporter>(&VendorSourceOptions {
        source: &source,
        packages: &packages,
        store_dir,
        git_shallow_hosts: &[],
        package_import_method: pnpm_config::PackageImportMethod::default(),
        logged_methods: &AtomicU8::new(0),
        offline,
    })
    .unwrap()
}

#[test]
fn a_workspace_member_is_vendored_without_the_repository_around_it() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (repository, commit) = commit_repository(
        temp_dir.path(),
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"member\"]\n\n[workspace.package]\nversion = \"0.3.0\"\n",
            ),
            ("member/Cargo.toml", "[package]\nname = \"member\"\nversion.workspace = true\n"),
            ("member/src/lib.rs", "pub fn answer() -> u8 { 42 }\n"),
        ],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();

    let linked = vendor_from(&repository, &commit, &store_dir, &[("member", "0.3.0")]);

    let (link_name, slot) = linked.first().expect("the member is vendored");
    assert_eq!(link_name, "member-0.3.0");
    // Trimmed: a checkout on Windows ends the line the way git configures
    // it to, not the way the fixture wrote it.
    assert_eq!(
        fs::read_to_string(slot.join("src/lib.rs")).unwrap().trim_end(),
        "pub fn answer() -> u8 { 42 }",
    );
    let manifest: toml::Table =
        toml::from_str(&fs::read_to_string(slot.join("Cargo.toml")).unwrap()).unwrap();
    assert_eq!(manifest["package"]["version"].as_str(), Some("0.3.0"));
    let checksum: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(slot.join(".cargo-checksum.json")).unwrap())
            .unwrap();
    assert_eq!(checksum["package"], serde_json::Value::Null);
    assert!(checksum["files"]["src/lib.rs"].is_string(), "{checksum}");
    assert!(!slot.join(".git").exists());
}

#[test]
fn a_root_package_leaves_the_members_nested_in_it_to_their_own_slots() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (repository, commit) = commit_repository(
        temp_dir.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"root\"\nversion = \"1.0.0\"\n\n[workspace]\nmembers = [\"member\"]\n",
            ),
            ("src/lib.rs", "pub fn root() {}\n"),
            ("member/Cargo.toml", "[package]\nname = \"member\"\nversion = \"0.3.0\"\n"),
            ("member/src/lib.rs", "pub fn member() {}\n"),
        ],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();

    let linked =
        vendor_from(&repository, &commit, &store_dir, &[("root", "1.0.0"), ("member", "0.3.0")]);

    let root = &linked.iter().find(|(name, _)| name == "root-1.0.0").expect("the root").1;
    assert!(root.join("src/lib.rs").is_file());
    assert!(!root.join("member").exists());
    assert!(!fs::read_to_string(root.join("Cargo.toml")).unwrap().contains("[workspace]"));
}

#[test]
fn a_vendored_package_is_taken_from_the_store_without_a_second_checkout() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (repository, commit) = commit_repository(
        temp_dir.path(),
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n"),
            ("src/lib.rs", "pub fn demo() {}\n"),
        ],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();
    let linked = vendor_from(&repository, &commit, &store_dir, &[("demo", "1.0.0")]);

    // Offline, so a second checkout of the repository would be an error.
    let relinked =
        vendor_from_offline(&repository, &commit, &store_dir, &[("demo", "1.0.0")], true);

    assert_eq!(relinked, linked);
}

#[test]
fn a_member_that_names_its_workspace_by_path_inherits_from_it() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (repository, commit) = commit_repository(
        temp_dir.path(),
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/member\"]\n\n[workspace.package]\nversion = \"0.3.0\"\n",
            ),
            (
                "crates/member/Cargo.toml",
                "[package]\nname = \"member\"\nworkspace = \"../..\"\nversion.workspace = true\n",
            ),
            ("crates/member/src/lib.rs", "pub fn member() {}\n"),
        ],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();

    let linked = vendor_from(&repository, &commit, &store_dir, &[("member", "0.3.0")]);

    let (_, slot) = linked.first().expect("the member is vendored");
    let manifest: toml::Table =
        toml::from_str(&fs::read_to_string(slot.join("Cargo.toml")).unwrap()).unwrap();
    assert_eq!(manifest["package"]["version"].as_str(), Some("0.3.0"));
}

#[cfg(unix)]
#[test]
fn a_symlinked_file_is_vendored_as_its_contents_unless_it_leaves_the_checkout() {
    let temp_dir = tempfile::tempdir().unwrap();
    let outside = temp_dir.path().join("outside");
    fs::write(&outside, "not repository content\n").unwrap();
    let repository = GitRepoFixture::init(temp_dir.path(), "repo");
    repository.write_file("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n");
    repository.write_file("LICENSE-MIT", "the license\n");
    repository.write_symlink("LICENSE", "LICENSE-MIT");
    repository.write_symlink("src/lib.rs", "../LICENSE-MIT");
    repository.write_symlink("escaped", "../outside");
    let commit = repository.commit("init");
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();

    let linked = vendor_from(&repository.file_url(), &commit, &store_dir, &[("demo", "1.0.0")]);

    let (_, slot) = linked.first().expect("the crate is vendored");
    assert_eq!(fs::read_to_string(slot.join("LICENSE")).unwrap(), "the license\n");
    assert_eq!(fs::read_to_string(slot.join("src/lib.rs")).unwrap(), "the license\n");
    assert!(!slot.join("escaped").exists());
}

#[test]
fn a_crate_is_found_past_a_manifest_that_shares_its_name_and_reads_no_version() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (repository, commit) = commit_repository(
        temp_dir.path(),
        &[
            // Sorts before `wanted`, so the scan reaches it first.
            ("fixture/Cargo.toml", "[package]\nname = \"demo\"\nversion.workspace = true\n"),
            ("wanted/Cargo.toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n"),
            ("wanted/src/lib.rs", "pub fn demo() {}\n"),
        ],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();

    let linked = vendor_from(&repository, &commit, &store_dir, &[("demo", "1.0.0")]);

    let (_, slot) = linked.first().expect("the crate is vendored");
    assert!(slot.join("src/lib.rs").is_file());
}

// APFS answers `EILSEQ` for a name that is not UTF-8, so macOS cannot
// hold the file this branch is about.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_file_whose_name_is_not_utf8_is_refused() {
    use super::{CheckoutPackage, import_package};
    use std::{collections::BTreeSet, ffi::OsStr, os::unix::ffi::OsStrExt as _};

    let temp_dir = tempfile::tempdir().unwrap();
    let package_dir = temp_dir.path().join("crate");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(package_dir.join(OsStr::from_bytes(b"lib\xff.rs")), "").unwrap();
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();

    let error = import_package(
        &store_dir,
        temp_dir.path(),
        &CheckoutPackage {
            dir: package_dir,
            manifest: "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n".to_string(),
        },
        &BTreeSet::new(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("not valid UTF-8"), "{error}");
}

#[test]
fn a_crate_the_checkout_does_not_hold_is_reported() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (repository, commit) = commit_repository(
        temp_dir.path(),
        &[("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n")],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();
    let source = Arc::new(
        GitSource::from_source_id(&source_id(&format!("git+{repository}#{commit}"))).unwrap(),
    );

    let error = vendor_source::<SilentReporter>(&VendorSourceOptions {
        source: &source,
        packages: &[GitPackage {
            name: "demo".to_string(),
            version: "2.0.0".to_string(),
            source: Arc::clone(&source),
        }],
        store_dir: &store_dir,
        git_shallow_hosts: &[],
        package_import_method: pnpm_config::PackageImportMethod::default(),
        logged_methods: &AtomicU8::new(0),
        offline: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("holds no crate demo 2.0.0"), "{error}");
}

#[test]
fn an_offline_install_does_not_check_out_a_missing_package() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    let source = Arc::new(
        GitSource::from_source_id(&source_id(
            "git+https://example.test/repo#1ae976a0023b4dec80b1a5411ccee8343f91320e",
        ))
        .unwrap(),
    );

    let error = vendor_source::<SilentReporter>(&VendorSourceOptions {
        source: &source,
        packages: &[GitPackage {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            source: Arc::clone(&source),
        }],
        store_dir: &store_dir,
        git_shallow_hosts: &[],
        package_import_method: pnpm_config::PackageImportMethod::default(),
        logged_methods: &AtomicU8::new(0),
        offline: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("while offline"), "{error}");
}
