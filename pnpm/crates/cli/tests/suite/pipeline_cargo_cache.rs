#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{
    fs::{self, File, FileTimes},
    path::Path,
    process::Command,
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git").current_dir(root).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn pnpm(root: &Path, cache: &Path, args: &[&str]) -> String {
    let result = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(root)
        .without_ambient_pnpm_config()
        .with_env("XDG_CACHE_HOME", cache)
        .with_env("XDG_CONFIG_HOME", cache.join("config"))
        .with_env("CARGO_INCREMENTAL", "1")
        .with_env("RUSTC_WRAPPER", "")
        .with_env("RUSTC_WORKSPACE_WRAPPER", "")
        .with_args(args)
        .assert()
        .success();
    let output = result.get_output();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn run_binary(root: &Path) -> String {
    let output = Command::new(root.join("target/debug/probe")).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cargo_state_is_shared_between_worktrees_and_survives_cache_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(temp.path()).unwrap();
    let first_worktree = root.join("a");
    let second_worktree = root.join("b");
    let cache = temp.path().join("cache");
    fs::create_dir_all(first_worktree.join("src")).unwrap();
    git(&first_worktree, &["init"]);
    fs::write(first_worktree.join(".gitignore"), "target/\nnode_modules/\n").unwrap();
    fs::write(
        first_worktree.join("Cargo.toml"),
        "[package]\nname = 'probe'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        first_worktree.join("src/main.rs"),
        "fn main() { println!(\"one\"); println!(\"{}\", env!(\"BUILD_ROOT\")); }\n",
    )
    .unwrap();
    fs::write(
        first_worktree.join("build.rs"),
        r#"fn main() {
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rustc-env=BUILD_ROOT={}", std::env::var("CARGO_MANIFEST_DIR").unwrap());
    }"#,
    )
    .unwrap();
    fs::write(first_worktree.join("package.json"), r#"{"name":"probe","version":"1.0.0","scripts":{"build":"cargo build --locked --offline && echo task-executed"}}"#).unwrap();
    fs::write(first_worktree.join("pnpm-workspace.yaml"), "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    cargoTargetDir: target\n").unwrap();
    assert!(
        Command::new("cargo")
            .current_dir(&first_worktree)
            .args(["generate-lockfile", "--offline"])
            .status()
            .unwrap()
            .success(),
    );
    pnpm(&first_worktree, &cache, &["install"]);
    git(&first_worktree, &["add", "."]);
    git(
        &first_worktree,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    );
    git(&first_worktree, &["worktree", "add", "--detach", second_worktree.to_str().unwrap()]);

    let first = pnpm(&first_worktree, &cache, &["pipeline", "--full"]);
    assert!(first.contains("task-executed"), "{first}");
    assert!(!first.contains("Cargo build cache:"), "{first}");
    let second = pnpm(&second_worktree, &cache, &["pipeline", "--full"]);
    assert!(second.contains("restored Cargo build state"), "{second}");
    assert!(second.contains("task-executed"), "{second}");
    assert_eq!(run_binary(&first_worktree), format!("one\n{}\n", first_worktree.display()));
    assert_eq!(run_binary(&second_worktree), format!("one\n{}\n", second_worktree.display()));

    let source = second_worktree.join("src/main.rs");
    let timestamp = fs::metadata(&source).unwrap().modified().unwrap();
    fs::write(
        &source,
        "fn main() { println!(\"two\"); println!(\"{}\", env!(\"BUILD_ROOT\")); }\n",
    )
    .unwrap();
    File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(FileTimes::new().set_modified(timestamp))
        .unwrap();
    pnpm(&second_worktree, &cache, &["pipeline", "--full"]);
    assert_eq!(run_binary(&second_worktree), format!("two\n{}\n", second_worktree.display()));
    assert_eq!(run_binary(&first_worktree), format!("one\n{}\n", first_worktree.display()));

    fs::remove_dir_all(cache.join("pnpm/cargo-build")).unwrap();
    assert_eq!(run_binary(&second_worktree), format!("two\n{}\n", second_worktree.display()));
    assert_eq!(run_binary(&first_worktree), format!("one\n{}\n", first_worktree.display()));
    let repeat = pnpm(&first_worktree, &cache, &["pipeline", "--full", "--no-cache"]);
    assert!(!cache.join("pnpm/cargo-build").exists());
    assert!(repeat.contains("task-executed"), "{repeat}");
    assert_eq!(run_binary(&first_worktree), format!("one\n{}\n", first_worktree.display()));
}
