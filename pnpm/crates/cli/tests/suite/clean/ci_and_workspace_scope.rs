use super::{CommandTempCwd, seed_package};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, path::Path, process::Command};

fn run_pnpm(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(cwd)
        .without_ambient_pnpm_config()
        .with_args(args)
        .output()
        .expect("run pnpm")
}

fn write_local_dep(workspace: &Path, name: &str) {
    let dep_dir = workspace.join("deps").join(name);
    fs::create_dir_all(&dep_dir).expect("create local dep dir");
    fs::write(
        dep_dir.join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
    )
    .expect("write local dep manifest");
}

#[test]
fn ci_preserves_node_modules_when_lockfile_is_missing_or_outdated() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");
    write_local_dep(&workspace, "dep-b");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("storeDir: {}\n", serde_json::to_string(&store_dir).expect("quote storeDir")),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"dep-a":"file:deps/dep-a"}}"#,
    )
    .expect("write package.json");

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");

    let marker = workspace.join("node_modules/keep-marker.txt");
    fs::write(&marker, "keep").expect("write marker in node_modules");
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let saved_lockfile = fs::read_to_string(&lockfile_path).expect("read lockfile");

    // 1. Missing pnpm-lock.yaml (even while node_modules/.pnpm/lock.yaml exists).
    fs::remove_file(&lockfile_path).expect("remove pnpm-lock.yaml");
    let missing_ci = run_pnpm(&workspace, &["ci"]);
    assert!(!missing_ci.status.success(), "ci without lockfile must fail");
    let missing_stderr = String::from_utf8_lossy(&missing_ci.stderr);
    assert!(
        missing_stderr.contains("ERR_PNPM_NO_LOCKFILE"),
        "expected ERR_PNPM_NO_LOCKFILE, got: {missing_stderr}",
    );
    assert!(marker.exists(), "marker must survive failed ci without lockfile");
    assert!(
        workspace.join("node_modules/dep-a/package.json").exists(),
        "installed dependency must survive failed ci without lockfile",
    );

    // 2. Outdated pnpm-lock.yaml relative to package.json.
    fs::write(&lockfile_path, &saved_lockfile).expect("restore lockfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"dep-a":"file:deps/dep-a","dep-b":"file:deps/dep-b"}}"#,
    )
    .expect("update package.json");
    let outdated_ci = run_pnpm(&workspace, &["ci"]);
    assert!(!outdated_ci.status.success(), "ci with outdated lockfile must fail");
    let outdated_stderr = String::from_utf8_lossy(&outdated_ci.stderr);
    assert!(
        outdated_stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
        "expected ERR_PNPM_OUTDATED_LOCKFILE, got: {outdated_stderr}",
    );
    assert!(marker.exists(), "marker must survive failed ci with outdated lockfile");
    assert!(
        workspace.join("node_modules/dep-a/package.json").exists(),
        "installed dependency must survive failed ci with outdated lockfile",
    );

    drop(root);
}

#[test]
fn ci_preserves_symlinked_node_modules_when_lockfile_is_missing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let external = root.path().join("external-nm");
    fs::create_dir_all(&external).expect("create external target");
    fs::write(external.join("keep.txt"), "keep").expect("write keep.txt");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"dep-a":"1.0.0"}}"#,
    )
    .expect("write package.json");
    let nm_link = workspace.join("node_modules");
    pnpm_fs::symlink_dir(&external, &nm_link).expect("symlink node_modules");

    let ci_out = run_pnpm(&workspace, &["ci"]);
    assert!(!ci_out.status.success(), "ci must fail without lockfile");
    let meta = fs::symlink_metadata(&nm_link).expect("symlinked node_modules must remain");
    assert!(
        pnpm_fs::is_symlink_metadata(&meta),
        "symlinked node_modules must not be unlinked when validation fails",
    );

    drop(root);
}

#[test]
fn ci_with_dedicated_lockfiles_preserves_all_node_modules_when_any_project_fails_validation() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");
    write_local_dep(&workspace, "dep-b");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nstoreDir: {}\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");

    let pkg_a = workspace.join("packages/pkg-a");
    let pkg_b = workspace.join("packages/pkg-b");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");
    fs::write(
        pkg_a.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-a":"file:../../deps/dep-a"}}"#,
    )
    .expect("write pkg-a manifest");
    fs::write(
        pkg_b.join("package.json"),
        r#"{"name":"pkg-b","version":"1.0.0","dependencies":{"dep-b":"file:../../deps/dep-b"}}"#,
    )
    .expect("write pkg-b manifest");

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");

    let marker_a = pkg_a.join("node_modules/marker-a.txt");
    let marker_b = pkg_b.join("node_modules/marker-b.txt");
    fs::write(&marker_a, "a").expect("write marker-a");
    fs::write(&marker_b, "b").expect("write marker-b");

    // Break only pkg-b's lockfile; pkg-a's node_modules must remain untouched too.
    fs::remove_file(pkg_b.join("pnpm-lock.yaml")).expect("remove pkg-b lockfile");

    let ci_out = run_pnpm(&workspace, &["ci"]);
    assert!(!ci_out.status.success(), "ci must fail when pkg-b lockfile is missing");
    assert!(marker_a.exists(), "pkg-a node_modules must survive when pkg-b fails validation");
    assert!(marker_b.exists(), "pkg-b node_modules must survive when pkg-b fails validation");

    drop(root);
}

#[test]
fn clean_and_ci_honor_filter_and_filter_prod_and_workspace_root() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");
    write_local_dep(&workspace, "dep-b");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nstoreDir: {}\nvirtualStoreDir: .virtual-store\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"root-pkg","version":"1.0.0","private":true,"dependencies":{"dep-a":"file:deps/dep-a"}}"#,
    )
    .expect("write root manifest");

    let pkg_a = workspace.join("packages/pkg-a");
    let pkg_b = workspace.join("packages/pkg-b");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");
    fs::write(
        pkg_a.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-a":"file:../../deps/dep-a"}}"#,
    )
    .expect("write pkg-a manifest");
    fs::write(
        pkg_b.join("package.json"),
        r#"{"name":"pkg-b","version":"1.0.0","dependencies":{"pkg-a":"workspace:*","dep-b":"file:../../deps/dep-b"}}"#,
    )
    .expect("write pkg-b manifest");

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");

    // 1. `pnpm --filter pkg-a clean` cleans only pkg-a, keeping pkg-b, root, and .virtual-store.
    let clean_filtered = run_pnpm(&workspace, &["--filter", "pkg-a", "clean"]);
    assert!(clean_filtered.status.success(), "{clean_filtered:?}");
    assert!(!pkg_a.join("node_modules/dep-a").exists(), "pkg-a dep should be cleaned");
    assert!(pkg_b.join("node_modules/dep-b").exists(), "pkg-b dep must remain");
    assert!(workspace.join("node_modules/dep-a").exists(), "root dep must remain");
    assert!(workspace.join(".virtual-store").exists(), "external virtual store must remain");

    // 2. `pnpm --filter pkg-a ci` cleans stale marker in pkg-a and re-materializes pkg-a without
    //    wiping pkg-b, root, or .virtual-store.
    fs::create_dir_all(pkg_a.join("node_modules")).expect("ensure pkg-a node_modules");
    let stale_a = pkg_a.join("node_modules/stale-a");
    fs::create_dir_all(&stale_a).expect("create stale-a");
    let marker_b = pkg_b.join("node_modules/keep-b");
    fs::create_dir_all(&marker_b).expect("create keep-b");

    let ci_filtered = run_pnpm(&workspace, &["--filter", "pkg-a", "ci"]);
    assert!(ci_filtered.status.success(), "{ci_filtered:?}");
    assert!(!stale_a.exists(), "stale entry in pkg-a must be removed by filtered ci");
    assert!(pkg_a.join("node_modules/dep-a/package.json").exists(), "pkg-a dep reinstalled");
    assert!(marker_b.exists(), "pkg-b marker must survive filtered ci");
    assert!(workspace.join(".virtual-store").exists(), "external virtual store must survive");

    // 3. `pnpm --filter-prod ...pkg-a clean` cleans pkg-a and its prod dependent pkg-b, keeping root.
    let clean_prod = run_pnpm(&workspace, &["--filter-prod", "...pkg-a", "clean"]);
    assert!(clean_prod.status.success(), "{clean_prod:?}");
    assert!(!pkg_a.join("node_modules/dep-a").exists(), "pkg-a cleaned by --filter-prod");
    assert!(!pkg_b.join("node_modules/dep-b").exists(), "pkg-b cleaned by --filter-prod");
    assert!(workspace.join("node_modules/dep-a").exists(), "root dep must survive --filter-prod");

    // 4. `pnpm -w clean` cleans only workspace root.
    seed_package(&pkg_a.join("node_modules"), "seeded-a");
    let clean_root = run_pnpm(&workspace, &["-w", "clean"]);
    assert!(clean_root.status.success(), "{clean_root:?}");
    assert!(!workspace.join("node_modules/dep-a").exists(), "root dep cleaned by -w");
    assert!(pkg_a.join("node_modules/seeded-a").exists(), "pkg-a untouched by -w clean");

    drop(root);
}

#[test]
fn clean_and_ci_clean_root_node_modules_in_workspace_without_root_manifest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let store_dir = root.path().join("store");
    write_local_dep(&workspace, "dep-a");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nstoreDir: {}\n",
            serde_json::to_string(&store_dir).expect("quote storeDir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    assert!(!workspace.join("package.json").exists(), "root must have no package.json");

    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(
        pkg_a.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"dep-a":"file:../../deps/dep-a"}}"#,
    )
    .expect("write pkg-a manifest");

    let install = run_pnpm(&workspace, &["install"]);
    assert!(install.status.success(), "{install:?}");
    assert!(workspace.join("node_modules/.pnpm").exists(), "root virtual store created");

    // Seed a stale package and a non-pnpm .cache in root node_modules.
    seed_package(&workspace.join("node_modules"), "stale-root-pkg");
    fs::create_dir_all(workspace.join("node_modules/.cache")).expect("create root .cache");
    fs::write(workspace.join("node_modules/.cache/keep"), "keep").expect("write .cache/keep");

    // Unfiltered `pnpm ci` cleans root node_modules (including stale-root-pkg) and reinstalls.
    let ci_out = run_pnpm(&workspace, &["ci"]);
    assert!(ci_out.status.success(), "{ci_out:?}");
    assert!(
        !workspace.join("node_modules/stale-root-pkg").exists(),
        "unfiltered ci must remove stale packages from root node_modules",
    );
    assert!(
        workspace.join("node_modules/.cache/keep").exists(),
        "non-pnpm .cache in root node_modules must be preserved",
    );
    assert!(pkg_a.join("node_modules/dep-a/package.json").exists(), "pkg-a reinstalled");

    // Unfiltered `pnpm clean` removes root node_modules/.pnpm as well as member node_modules.
    let clean_out = run_pnpm(&workspace, &["clean"]);
    assert!(clean_out.status.success(), "{clean_out:?}");
    assert!(
        !workspace.join("node_modules/.pnpm").exists(),
        "unfiltered clean must remove root node_modules/.pnpm without root package.json",
    );
    assert!(
        !pkg_a.join("node_modules/dep-a").exists(),
        "unfiltered clean must remove member node_modules",
    );

    drop(root);
}

#[test]
fn clean_lockfile_respects_lockfile_dir_containment_and_per_project_lockfiles() {
    // 1. Custom lockfileDir inside workspace root (via pnpm-workspace.yaml and via --lockfile-dir).
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nlockfileDir: config/lock\n",
    )
    .expect("write pnpm-workspace.yaml");
    let lock_dir = workspace.join("config/lock");
    fs::create_dir_all(lock_dir.join("node_modules/.pnpm")).expect("create lockfileDir .pnpm");
    fs::write(lock_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").expect("write lockfile");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a"}"#).expect("write pkg-a manifest");

    let clean_manifest_lock = run_pnpm(&workspace, &["clean", "--lockfile"]);
    assert!(clean_manifest_lock.status.success(), "{clean_manifest_lock:?}");
    assert!(
        !lock_dir.join("pnpm-lock.yaml").exists(),
        "lockfile inside configured lockfileDir must be removed",
    );
    assert!(
        !lock_dir.join("node_modules/.pnpm").exists(),
        "virtual store inside configured lockfileDir must be cleaned",
    );

    let cli_lock_dir = workspace.join("cli-lock");
    fs::create_dir_all(&cli_lock_dir).expect("create cli-lock");
    fs::write(cli_lock_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write cli lockfile");
    let clean_cli_lock =
        run_pnpm(&workspace, &["clean", "--lockfile", "--lockfile-dir", "cli-lock"]);
    assert!(clean_cli_lock.status.success(), "{clean_cli_lock:?}");
    assert!(
        !cli_lock_dir.join("pnpm-lock.yaml").exists(),
        "--lockfile-dir inside root must be honored by clean --lockfile",
    );

    // 2. Containment: external lockfileDir and symlinked-parent escape must be left untouched.
    let outside = root.path().join("outside-lock");
    fs::create_dir_all(outside.join("sub")).expect("create outside-lock/sub");
    fs::write(outside.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write outside lockfile");
    fs::write(outside.join("sub/pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write outside sub lockfile");
    pnpm_fs::symlink_dir(&outside.join("sub"), &workspace.join("escape"))
        .expect("symlink escape -> outside-lock/sub");
    let keep_dir = workspace.join("keep-dir");
    fs::create_dir_all(&keep_dir).expect("create keep-dir");
    fs::write(keep_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
        .expect("write keep-dir lockfile");

    for escaped in ["../outside-lock", "escape", "escape/../keep-dir"] {
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!(
                "packages:\n  - packages/*\nlockfileDir: {}\n",
                serde_json::to_string(escaped).expect("quote lockfileDir"),
            ),
        )
        .expect("write escaped lockfileDir");
        let res = run_pnpm(&workspace, &["clean", "--lockfile"]);
        assert!(res.status.success(), "clean failed for {escaped}: {res:?}");
        assert!(outside.join("pnpm-lock.yaml").exists(), "outside lockfile deleted for {escaped}");
        assert!(
            outside.join("sub/pnpm-lock.yaml").exists(),
            "outside sub lockfile deleted for {escaped}",
        );
        assert!(
            keep_dir.join("pnpm-lock.yaml").exists(),
            "symlink-parent traversal lockfile deleted for {escaped}",
        );
    }

    // 3. Per-project lockfiles under sharedWorkspaceLockfile: false (filtered and unfiltered).
    let pkg_b = workspace.join("packages/pkg-b");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");
    fs::write(pkg_b.join("package.json"), r#"{"name":"pkg-b"}"#).expect("write pkg-b manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write sharedWorkspaceLockfile: false");
    fs::write(pkg_a.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").expect("write pkg-a lock");
    fs::write(pkg_b.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").expect("write pkg-b lock");

    let filtered_lock_clean = run_pnpm(&workspace, &["--filter", "pkg-a", "clean", "--lockfile"]);
    assert!(filtered_lock_clean.status.success(), "{filtered_lock_clean:?}");
    assert!(!pkg_a.join("pnpm-lock.yaml").exists(), "filtered pkg-a lockfile removed");
    assert!(pkg_b.join("pnpm-lock.yaml").exists(), "unselected pkg-b lockfile preserved");

    let all_lock_clean = run_pnpm(&workspace, &["clean", "--lockfile"]);
    assert!(all_lock_clean.status.success(), "{all_lock_clean:?}");
    assert!(!pkg_b.join("pnpm-lock.yaml").exists(), "unfiltered per-project lockfile removed");

    drop(root);
}
