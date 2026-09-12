use super::{
    AddMockedRegistry, CommandTempCwd, GitRepoFixture, IS_POSITIVE_BINDING_GYP_PATCH,
    IS_POSITIVE_HOOKS_FILE_PATCH, IS_POSITIVE_POSTINSTALL_PATCH, MARKER_PATCH, Value,
    append_workspace_yaml_key, assert_patch_apply_failure, assert_patch_install_scenario, fs,
    is_positive_store_row, pacquet, patch_file_hash, read_installed_index, read_wanted_lockfile,
    remove_dir_if_exists, setup_configured_patch, setup_configured_patch_with_yaml, snapshot_keys,
};
use assert_cmd::assert::OutputAssertExt;

/// TS: `patch package with exact version` (`patch.ts:24`).
#[test]
fn install_level_exact_version_patch_applies_with_frozen_reinstall() {
    assert_patch_install_scenario("is-positive@1.0.0", "is-positive@1.0.0.patch", "");
}

/// TS: `patch package with version range` (`patch.ts:120`).
#[test]
fn install_level_range_patch_applies_with_frozen_reinstall() {
    assert_patch_install_scenario("is-positive@1", "is-positive@1.patch", "");
}

/// TS: `patch package when scripts are ignored` (`patch.ts:297`).
#[test]
fn install_level_patch_applies_when_scripts_are_ignored() {
    assert_patch_install_scenario(
        "is-positive@1.0.0",
        "is-positive@1.0.0.patch",
        "ignoreScripts: true\n",
    );
}

/// TS: `patch package when the package is not in allowBuilds list`
/// (`patch.ts:386`). An empty `allowBuilds` forbids every build, but a
/// patch is not a build — it still applies.
#[test]
fn install_level_patch_applies_when_the_package_is_not_in_allow_builds() {
    assert_patch_install_scenario(
        "is-positive@1.0.0",
        "is-positive@1.0.0.patch",
        "allowBuilds: {}\n",
    );
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/14648>.
#[test]
fn install_level_patch_that_adds_install_scripts_asks_for_approval() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("patches").join("is-positive@1.0.0.patch"),
        IS_POSITIVE_POSTINSTALL_PATCH,
    )
    .expect("write the postinstall patch");
    let marker = workspace.join("node_modules/is-positive/postinstall-ran.txt");

    let output = pacquet(&workspace, ["install"]).output().expect("run install");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("unapproved install:\n{combined}");
    assert!(!output.status.success(), "an unapproved build must fail under strictDepBuilds");
    // The package name is not matched here: the diagnostic wraps it
    // across lines. The `allowBuilds` entry asserted below names it.
    assert!(
        combined.contains("ERR_PNPM_IGNORED_BUILDS") && combined.contains("Ignored build scripts"),
        "expected the patched package to be reported as an ignored build; got:\n{combined}",
    );
    assert!(!marker.exists(), "the postinstall must not run before it is approved");

    // The failed install left an `allowBuilds` entry for the user to
    // decide on; answering it is what `pnpm approve-builds` writes.
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    assert!(
        yaml.contains("is-positive: set this to true or false"),
        "expected an undecided allowBuilds entry; got:\n{yaml}",
    );
    fs::write(&yaml_path, yaml.replace("set this to true or false", "true"))
        .expect("write pnpm-workspace.yaml");
    remove_dir_if_exists(&workspace.join("node_modules"));
    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    assert!(marker.exists(), "the approved postinstall must run");

    drop((root, mock_instance));
}

/// A patched package with no build to run caches its overlay under a
/// dep-graph-free key, which `--ignore-scripts` is the cheapest way to
/// produce. Deciding `requiresBuild` any later than that key is composed
/// reads the key back and skips the build the approval just allowed.
#[test]
fn install_level_patch_that_adds_install_scripts_outlives_a_pre_fix_cache_entry() {
    let (root, workspace, npmrc_info) = setup_configured_patch_with_yaml(
        "is-positive@1.0.0",
        "is-positive@1.0.0.patch",
        "allowBuilds:\n  is-positive: true\n",
    );
    let AddMockedRegistry { mock_instance, store_dir, .. } = npmrc_info;
    fs::write(
        workspace.join("patches").join("is-positive@1.0.0.patch"),
        IS_POSITIVE_POSTINSTALL_PATCH,
    )
    .expect("write the postinstall patch");
    let marker = workspace.join("node_modules/is-positive/postinstall-ran.txt");

    pacquet(&workspace, ["install", "--ignore-scripts", "--reporter=silent"]).assert().success();
    assert!(!marker.exists(), "--ignore-scripts must not run the postinstall");
    let cache_keys: Vec<String> = is_positive_store_row(&store_dir)
        .side_effects
        .expect("a patched package populates `sideEffects`")
        .into_keys()
        .collect();
    assert!(
        cache_keys.iter().any(|key| !key.contains(";deps=")),
        "the store must hold the dep-graph-free key a pre-fix pnpm 12 wrote; got {cache_keys:?}",
    );

    remove_dir_if_exists(&workspace.join("node_modules"));
    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    assert!(marker.exists(), "the pre-fix cache entry must not suppress the build");

    drop((root, mock_instance));
}

/// A patch can introduce build work without touching `scripts`: the
/// lifecycle runner reads a `binding.gyp` as `node-gyp rebuild`.
#[test]
fn install_level_patch_that_adds_a_binding_gyp_asks_for_approval() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("patches").join("is-positive@1.0.0.patch"),
        IS_POSITIVE_BINDING_GYP_PATCH,
    )
    .expect("write the binding.gyp patch");

    let output = pacquet(&workspace, ["install"]).output().expect("run install");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("unapproved install:\n{combined}");
    assert!(!output.status.success(), "an unapproved native build must fail the install");
    assert!(
        combined.contains("ERR_PNPM_IGNORED_BUILDS"),
        "expected the patched package to be reported as an ignored build; got:\n{combined}",
    );

    drop((root, mock_instance));
}

/// Only entries below a `.hooks` directory are hooks, so a plain file by
/// that name must not hold the install for an approval it does not need.
#[test]
fn install_level_patch_that_adds_a_hooks_file_does_not_ask_for_approval() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("patches").join("is-positive@1.0.0.patch"),
        IS_POSITIVE_HOOKS_FILE_PATCH,
    )
    .expect("write the .hooks file patch");

    let output = pacquet(&workspace, ["install"]).output().expect("run install");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("install:\n{combined}");
    assert!(output.status.success(), "a plain `.hooks` file must not fail the install");
    assert!(
        !combined.contains("ERR_PNPM_IGNORED_BUILDS"),
        "the package must not be reported as an ignored build; got:\n{combined}",
    );
    let hooks = workspace.join("node_modules/is-positive/.hooks");
    assert!(hooks.is_file(), "the patch writes a plain file");

    drop((root, mock_instance));
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/14273>.
#[test]
fn git_dependency_patch_applies_on_fresh_and_frozen_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, store_dir, cache_dir, .. } = npmrc_info;
    let repo = GitRepoFixture::init(root.path(), "patched-git-dependency");
    repo.write_file(
        "package.json",
        &serde_json::json!({
            "name": "is-positive",
            "version": "3.1.0",
            "main": "index.js",
        })
        .to_string(),
    );
    repo.write_file("index.js", "module.exports = true\n");
    let commit = repo.commit("initial package");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": repo.git_url_at(&commit),
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/is-positive@3.1.0.patch"), MARKER_PATCH)
        .expect("write patch file");
    append_workspace_yaml_key(
        &workspace,
        "patchedDependencies",
        "\n  is-positive@3.1.0: patches/is-positive@3.1.0.patch",
    );

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    let marker = workspace.join("node_modules/is-positive/patched-marker.txt");
    assert_eq!(fs::read_to_string(&marker).expect("read fresh marker"), "patched\n");
    let patch_hash = patch_file_hash(&workspace, "is-positive@3.1.0.patch");
    let snapshots = snapshot_keys(&read_wanted_lockfile(&workspace));
    assert!(
        snapshots.iter().any(|key| key.contains(&format!("(patch_hash={patch_hash})"))),
        "snapshots: {snapshots:?}",
    );

    remove_dir_if_exists(&workspace.join("node_modules"));
    remove_dir_if_exists(&store_dir);
    remove_dir_if_exists(&cache_dir);
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read frozen marker"), "patched\n");

    drop((root, mock_instance));
}

/// TS: `the patched package is updated if the patch is modified`
/// (`patch.ts:269`).
#[test]
fn install_level_modified_patch_is_reapplied() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    let installed = read_installed_index(&workspace);
    assert!(installed.contains("// patched"), "installed: {installed}");

    let patch_path = workspace.join("patches/is-positive@1.0.0.patch");
    let patch = fs::read_to_string(&patch_path).expect("read the patch file");
    fs::write(&patch_path, patch.replace("// patched", "// edited patch"))
        .expect("rewrite the patch file");

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    let updated = read_installed_index(&workspace);
    assert!(updated.contains("// edited patch"), "updated: {updated}");

    drop((root, mock_instance));
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/13307>.
#[test]
fn install_reads_patched_dependencies_written_by_pnpm_10() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let patch_hash = patch_file_hash(&workspace, "is-positive@1.0.0.patch");
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile_text = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let pnpm_10_text = lockfile_text.replace(
        &format!("  is-positive@1.0.0: {patch_hash}\n"),
        &format!(
            "  is-positive@1.0.0:\n    hash: {patch_hash}\n    path: patches/is-positive@1.0.0.patch\n",
        ),
    );
    assert_ne!(pnpm_10_text, lockfile_text, "lockfile: {lockfile_text}");
    fs::write(&lockfile_path, &pnpm_10_text).expect("write the pnpm 10 lockfile");

    remove_dir_if_exists(&workspace.join("node_modules"));
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
    let frozen = read_installed_index(&workspace);
    assert!(frozen.contains("// patched"), "frozen: {frozen}");
    assert_eq!(
        fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml"),
        pnpm_10_text,
        "a frozen install must leave the lockfile alone",
    );

    remove_dir_if_exists(&workspace.join("node_modules"));
    let patch_path = workspace.join("patches/is-positive@1.0.0.patch");
    let patch = fs::read_to_string(&patch_path).expect("read the patch file");
    fs::write(&patch_path, patch.replace("// patched", "// edited patch"))
        .expect("rewrite the patch file");
    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    let installed = read_installed_index(&workspace);
    assert!(installed.contains("// edited patch"), "installed: {installed}");

    let edited_hash = patch_file_hash(&workspace, "is-positive@1.0.0.patch");
    let rewritten = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(
        rewritten,
        lockfile_text.replace(&patch_hash, &edited_hash),
        "an install that rewrites the lockfile normalizes the entry to the bare hash",
    );

    drop((root, mock_instance));
}

/// TS: `patch package when the patched package has no dependencies and
/// appears multiple times` (`patch.ts:475`). `is-not-positive` depends on
/// `is-positive@^3.1.0`, which the override pins back onto the patched
/// `1.0.0`, so the patched package is reached twice yet resolves to a
/// single snapshot.
#[test]
fn install_level_patch_applies_to_a_package_reached_multiple_times() {
    let (root, workspace, npmrc_info) = setup_configured_patch_with_yaml(
        "is-positive@1.0.0",
        "is-positive@1.0.0.patch",
        "overrides:\n  is-positive: 1.0.0\n",
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
                "is-not-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("rewrite package.json");

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let installed = read_installed_index(&workspace);
    assert!(installed.contains("// patched"), "installed: {installed}");
    let patch_hash = patch_file_hash(&workspace, "is-positive@1.0.0.patch");
    assert_eq!(
        snapshot_keys(&read_wanted_lockfile(&workspace)),
        vec![
            "is-not-positive@1.0.0".to_string(),
            format!("is-positive@1.0.0(patch_hash={patch_hash})"),
        ],
    );

    drop((root, mock_instance));
}

/// The hoisted linker nests a second copy of a package under each
/// consumer when a version conflict keeps it out of the root, and every
/// copy has to carry the patch — the TypeScript CLI patches all of them.
/// `send` and `finalhandler` both need `debug@2.6.9` while the root pins
/// `debug@4.3.4`, so `2.6.9` nests twice.
///
/// The frozen replay covers the other half: it restores the package from
/// the side-effects cache instead of re-running the patch, so the cached
/// overlay has to reach every copy too.
#[test]
fn hoisted_patch_reaches_every_nested_copy_of_a_package() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "send": "0.17.1",
                "finalhandler": "1.1.2",
                "debug": "4.3.4",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/debug.patch"), MARKER_PATCH).expect("write patch file");
    append_workspace_yaml_key(&workspace, "nodeLinker", "hoisted");
    append_workspace_yaml_key(
        &workspace,
        "patchedDependencies",
        "\n  debug@2.6.9: patches/debug.patch",
    );

    let nested_copies =
        [workspace.join("node_modules/send"), workspace.join("node_modules/finalhandler")];

    for frozen in [false, true] {
        remove_dir_if_exists(&workspace.join("node_modules"));
        let mut args = vec!["install", "--reporter=silent"];
        if frozen {
            args.push("--frozen-lockfile");
        }
        pacquet(&workspace, args).assert().success();

        for consumer in &nested_copies {
            let nested = consumer.join("node_modules/debug");
            assert_eq!(
                fs::read_to_string(nested.join("package.json"))
                    .ok()
                    .and_then(|manifest| serde_json::from_str::<Value>(&manifest).ok())
                    .and_then(|manifest| manifest["version"].as_str().map(ToOwned::to_owned)),
                Some("2.6.9".to_string()),
                "expected the conflicting debug@2.6.9 to nest under {}",
                consumer.display(),
            );
            assert!(
                nested.join("patched-marker.txt").is_file(),
                "unpatched nested copy at {} (frozen: {frozen})",
                nested.display(),
            );
        }
    }

    drop((root, mock_instance));
}

/// TS: `patch package should fail when the exact version patch fails to
/// apply` (`patch.ts:508`).
#[test]
fn install_level_exact_version_patch_that_does_not_apply_fails() {
    assert_patch_apply_failure("is-positive@3.1.0");
}

/// TS: `patch package should fail when the version range patch fails to
/// apply` (`patch.ts:530`).
#[test]
fn install_level_range_patch_that_does_not_apply_fails() {
    assert_patch_apply_failure("is-positive@>=3");
}

/// TS: `patch package should fail when the name-only range patch fails to
/// apply` (`patch.ts:552`).
#[test]
fn install_level_name_only_patch_that_does_not_apply_fails() {
    assert_patch_apply_failure("is-positive");
}
