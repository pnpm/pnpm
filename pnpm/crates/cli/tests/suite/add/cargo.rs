use super::{
    Command, CommandExtra, TempDir, assert_eq, cache_foo_index, cargo_add_project, prod_spec,
};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

#[test]
fn add_crate_updates_cargo_without_creating_a_node_manifest() {
    let (root, cache_dir) = cargo_add_project();
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args(["add", "crate:foo", "--offline", "--lockfile-only"])
        .assert()
        .success();

    let manifest = std::fs::read_to_string(root.path().join("Cargo.toml"))
        .expect("read updated Cargo manifest");
    assert!(
        manifest.contains(
            r#"[dependencies]
foo = "1.0.0""#,
        ),
        "{manifest}",
    );
    let lockfile = std::fs::read_to_string(root.path().join("Cargo.lock"))
        .expect("read updated Cargo lockfile");
    assert!(lockfile.contains(r#"name = "foo""#), "{lockfile}");
    assert!(!root.path().join("package.json").exists());
    assert!(!root.path().join("pnpm-lock.yaml").exists());
}

#[test]
fn add_crate_can_target_build_dependencies() {
    let (root, cache_dir) = cargo_add_project();
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args(["add", "crate:foo@1", "--save-build", "--offline", "--lockfile-only"])
        .assert()
        .success();

    let manifest = std::fs::read_to_string(root.path().join("Cargo.toml"))
        .expect("read updated Cargo manifest");
    assert!(manifest.contains("[build-dependencies]\nfoo = \"1\""), "{manifest}");
}

#[test]
fn mixed_add_updates_node_and_cargo_projects_together() {
    let (root, cache_dir) = cargo_add_project();
    let local_package = root.path().join("local-package");
    std::fs::create_dir(&local_package).expect("create local npm package");
    std::fs::write(
        local_package.join("package.json"),
        r#"{"name":"local-package","version":"1.0.0"}"#,
    )
    .expect("write local npm manifest");
    std::fs::write(root.path().join("package.json"), r#"{"name":"app","version":"1.0.0"}"#)
        .expect("write root npm manifest");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args([
            "add",
            "local-package@file:./local-package",
            "crate:foo@1",
            "--offline",
            "--lockfile-only",
        ])
        .assert()
        .success();

    assert_eq!(prod_spec(root.path(), "local-package"), "file:./local-package");
    let cargo_manifest =
        std::fs::read_to_string(root.path().join("Cargo.toml")).expect("read Cargo manifest");
    assert!(cargo_manifest.contains(r#"foo = "1""#), "{cargo_manifest}");
    assert!(root.path().join("pnpm-lock.yaml").is_file());
    assert!(root.path().join("Cargo.lock").is_file());
}

#[test]
fn mixed_add_restores_metadata_when_an_ecosystem_fails() {
    let (root, cache_dir) = cargo_add_project();
    let local_package = root.path().join("local-package");
    std::fs::create_dir(&local_package).expect("create local npm package");
    std::fs::write(
        local_package.join("package.json"),
        r#"{"name":"local-package","version":"1.0.0"}"#,
    )
    .expect("write local npm manifest");
    let node_manifest_path = root.path().join("package.json");
    let cargo_manifest_path = root.path().join("Cargo.toml");
    let cargo_lock_path = root.path().join("Cargo.lock");
    let node_manifest = r#"{"name":"app","version":"1.0.0"}"#;
    std::fs::write(&node_manifest_path, node_manifest).expect("write root npm manifest");
    let cargo_manifest = std::fs::read(&cargo_manifest_path).expect("snapshot Cargo manifest");
    let cargo_lock = std::fs::read(&cargo_lock_path).expect("snapshot Cargo lockfile");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args(["add", "local-package@file:./local-package", "crate:foo@1", "--offline"])
        .assert()
        .failure();

    assert_eq!(std::fs::read_to_string(node_manifest_path).unwrap(), node_manifest);
    assert_eq!(std::fs::read(cargo_manifest_path).unwrap(), cargo_manifest);
    assert_eq!(std::fs::read(cargo_lock_path).unwrap(), cargo_lock);
    eprintln!("wanted lockfile after rollback: {}", root.path().join("pnpm-lock.yaml").display());
    assert!(!root.path().join("pnpm-lock.yaml").exists());
    eprintln!(
        "current lockfile after rollback: {}",
        root.path().join("node_modules/.pnpm/lock.yaml").display(),
    );
    assert!(!root.path().join("node_modules/.pnpm/lock.yaml").exists());
    eprintln!(
        "modules manifest after rollback: {}",
        root.path().join("node_modules/.modules.yaml").display(),
    );
    assert!(!root.path().join("node_modules/.modules.yaml").exists());
}

#[test]
fn add_crate_in_a_cargo_workspace_member_updates_the_workspace_lockfile() {
    let root = TempDir::new().expect("create Cargo workspace");
    let cache_dir = root.path().join("cache");
    cache_foo_index(&cache_dir);
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .expect("write Cargo workspace manifest");
    std::fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "packages:\n  - member\ncargo:\n  enabled: true\n",
    )
    .expect("enable Cargo dependency management");
    let member = root.path().join("member");
    std::fs::create_dir_all(member.join("src")).expect("create member source directory");
    std::fs::write(member.join("src/lib.rs"), "").expect("write member source");
    std::fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write member Cargo manifest");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&member)
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args(["add", "crate:foo@1", "--offline", "--lockfile-only"])
        .assert()
        .success();

    let member_manifest =
        std::fs::read_to_string(member.join("Cargo.toml")).expect("read member Cargo manifest");
    assert!(member_manifest.contains(r#"foo = "1""#), "{member_manifest}");
    assert!(root.path().join("Cargo.lock").is_file());
    assert!(!member.join("Cargo.lock").exists());
    assert!(!member.join("package.json").exists());
}

#[test]
fn add_crate_uses_a_nested_cargo_workspace_root() {
    let root = TempDir::new().expect("create pnpm workspace");
    let cache_dir = root.path().join("cache");
    cache_foo_index(&cache_dir);
    std::fs::write(root.path().join("package.json"), r#"{"name":"repository"}"#)
        .expect("write pnpm root manifest");
    std::fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "packages:\n  - rust/member\ncargo:\n  enabled: true\n",
    )
    .expect("enable Cargo dependency management");
    let cargo_root = root.path().join("rust");
    let member = cargo_root.join("member");
    std::fs::create_dir_all(member.join("src")).expect("create member source directory");
    std::fs::write(
        cargo_root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .expect("write nested Cargo workspace manifest");
    std::fs::write(member.join("src/lib.rs"), "").expect("write member source");
    std::fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write member Cargo manifest");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&member)
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args(["add", "crate:foo@1", "--offline", "--lockfile-only"])
        .assert()
        .success();

    assert!(cargo_root.join("Cargo.lock").is_file());
    assert!(!root.path().join("Cargo.lock").exists());
    assert!(!member.join("Cargo.lock").exists());
}

#[test]
fn install_discovers_multiple_nested_cargo_workspaces() {
    let root = TempDir::new().expect("create pnpm workspace");
    std::fs::write(root.path().join("package.json"), r#"{"name":"repository"}"#)
        .expect("write pnpm root manifest");
    std::fs::write(root.path().join("pnpm-workspace.yaml"), "cargo:\n  enabled: true\n")
        .expect("enable Cargo dependency management");
    for project_name in ["rust-a", "rust-b"] {
        let project = root.path().join(project_name);
        std::fs::create_dir_all(project.join("src")).expect("create Cargo source directory");
        std::fs::write(project.join("src/lib.rs"), "").expect("write Cargo source");
        std::fs::write(
            project.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
        )
        .expect("write Cargo manifest");
    }

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_args(["install", "--offline", "--lockfile-only"])
        .assert()
        .success();

    assert!(root.path().join("rust-a/Cargo.lock").is_file());
    assert!(root.path().join("rust-b/Cargo.lock").is_file());
    assert!(!root.path().join("Cargo.lock").exists());
}
