use super::{
    GitPackage, GitSource, Manifest, VendorSourceOptions, vendor_source, vendored_package,
};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::StoreDir;
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{Arc, atomic::AtomicU8},
};

fn manifest(text: &str) -> Manifest {
    Manifest { text: text.to_string(), document: toml::from_str(text).expect("parse manifest") }
}

fn source_id(source: &str) -> cargo_lock::SourceId {
    source.parse().expect("parse Cargo source")
}

fn git(args: &[&str], cwd: &Path) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("run git {args:?}: {error}"));
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("decode git output")
}

/// Commit `files` into a fresh repository and answer its commit hash.
fn commit_repository(dir: &Path, files: &[(&str, &str)]) -> String {
    fs::create_dir_all(dir).unwrap();
    git(&["init", "-q", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.invalid"], dir);
    git(&["config", "user.name", "Test"], dir);
    for (path, contents) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    git(&["add", "-A"], dir);
    git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], dir);
    git(&["rev-parse", "HEAD"], dir).trim().to_string()
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
fn a_source_without_a_locked_commit_is_refused() {
    let error = GitSource::from_source_id(&source_id("git+https://example.test/repo?branch=next"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("pins no commit"), "{error}");
}

fn vendor_from(
    repository: &Path,
    commit: &str,
    store_dir: &StoreDir,
    packages: &[(&str, &str)],
) -> Vec<(String, std::path::PathBuf)> {
    let source = Arc::new(
        GitSource::from_source_id(&source_id(&format!(
            "git+file://{}#{commit}",
            repository.display(),
        )))
        .unwrap(),
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
        offline: false,
    })
    .unwrap()
}

#[test]
fn a_workspace_member_is_vendored_without_the_repository_around_it() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repository = temp_dir.path().join("repo");
    let commit = commit_repository(
        &repository,
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
    assert_eq!(
        fs::read_to_string(slot.join("src/lib.rs")).unwrap(),
        "pub fn answer() -> u8 { 42 }\n",
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
    let repository = temp_dir.path().join("repo");
    let commit = commit_repository(
        &repository,
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
    let repository = temp_dir.path().join("repo");
    let commit = commit_repository(
        &repository,
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n"),
            ("src/lib.rs", "pub fn demo() {}\n"),
        ],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();
    let linked = vendor_from(&repository, &commit, &store_dir, &[("demo", "1.0.0")]);
    fs::remove_dir_all(&repository).unwrap();

    assert_eq!(vendor_from(&repository, &commit, &store_dir, &[("demo", "1.0.0")]), linked);
}

#[test]
fn a_crate_the_checkout_does_not_hold_is_reported() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repository = temp_dir.path().join("repo");
    let commit = commit_repository(
        &repository,
        &[("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n")],
    );
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    store_dir.init().unwrap();
    let source = Arc::new(
        GitSource::from_source_id(&source_id(&format!(
            "git+file://{}#{commit}",
            repository.display(),
        )))
        .unwrap(),
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
