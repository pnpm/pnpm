use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
#[cfg(unix)]
use std::path::Path;
use std::{
    fs,
    path::{Component, PathBuf},
};

#[test]
fn should_list_registries() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    fs::create_dir_all(cache_dir.join("registry.npmjs.org")).unwrap();
    fs::create_dir_all(cache_dir.join("registry.yarnpkg.com")).unwrap();

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("list-registries")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("registry.npmjs.org"));
    assert!(stdout.contains("registry.yarnpkg.com"));
}

/// `cache view` labels a registry with its decoded URL, so listing must too.
/// Printing the raw key left the two commands disagreeing about what a
/// registry is called.
#[test]
fn should_list_registries_as_decoded_urls() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    for registry in ["https://registry.npmjs.org/", "https://npm.example:8443/team/a/"] {
        let registry_name =
            pnpm_resolving_npm_resolver::mirror::get_registry_name(registry).unwrap();
        fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    }

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("list-registries")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let listed: Vec<&str> = stdout.lines().collect();
    assert_eq!(listed, ["https://npm.example:8443/team/a/", "https://registry.npmjs.org/"]);
}

#[test]
fn should_list_packages() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name = pnpm_resolving_npm_resolver::mirror::get_registry_name(url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), "{}").unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-negative.jsonl"), "{}").unwrap();

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains(&format!("{registry_name}/is-positive.jsonl")));
    assert!(stdout.contains(&format!("{registry_name}/is-negative.jsonl")));
}

#[test]
fn should_list_only_files_not_directories() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name = pnpm_resolving_npm_resolver::mirror::get_registry_name(url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), "{}").unwrap();
    // A scoped package lives in its own directory, which the glob also matches.
    // Only the file underneath it, not the directory itself, should be listed.
    fs::create_dir_all(cache_dir.join(&registry_name).join("@scope")).unwrap();
    fs::write(
        cache_dir
            .join(&registry_name)
            .join("@scope")
            .join("foo.jsonl"),
        "{}",
    )
    .unwrap();

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains(&format!("{registry_name}/is-positive.jsonl")));
    assert!(stdout.contains(&format!("{registry_name}/@scope/foo.jsonl")));
    let scope_dir = format!("{registry_name}/@scope");
    assert!(
        !stdout
            .lines()
            .any(|line| line == scope_dir),
        "directory entry {scope_dir:?} should not be listed, got: {stdout}",
    );
}

#[test]
fn should_delete_packages() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name = pnpm_resolving_npm_resolver::mirror::get_registry_name(url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), "{}").unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-negative.jsonl"), "{}").unwrap();

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("delete")
        .with_arg("is-positive")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains(&format!("{registry_name}/is-positive.jsonl")));
    assert!(
        !cache_dir
            .join(&registry_name)
            .join("is-positive.jsonl")
            .exists(),
    );
    assert!(
        cache_dir
            .join(&registry_name)
            .join("is-negative.jsonl")
            .exists(),
    );
}

#[test]
fn should_delete_packages_from_all_metadata_dirs() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name = pnpm_resolving_npm_resolver::mirror::get_registry_name(url_str).unwrap();
    // A package can be cached under any metadata directory depending on the
    // resolution mode used at fetch time, so all of them must be cleared.
    let meta_dirs = [
        pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR,
        pnpm_resolving_npm_resolver::mirror::FULL_META_DIR,
        pnpm_resolving_npm_resolver::mirror::FULL_FILTERED_META_DIR,
    ];
    for meta_dir in meta_dirs {
        let dir = cwd.npmrc_info.cache_dir.join(meta_dir).join(&registry_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("is-positive.jsonl"), "{}").unwrap();
    }

    cwd.pacquet
        .with_arg("cache")
        .with_arg("delete")
        .with_arg("is-positive")
        .assert()
        .success();

    for meta_dir in meta_dirs {
        let file = cwd.npmrc_info.cache_dir
            .join(meta_dir)
            .join(&registry_name)
            .join("is-positive.jsonl");
        assert!(!file.exists(), "expected {file:?} to be deleted");
    }
}

/// A registry whose key carries the scheme, so `is_unreadable_registry_key`
/// spares it.
const LIVE_REGISTRY: &str = "https://registry.example/";

/// Upgrading past the host-only cache key strands the directory it wrote:
/// the registry now resolves to a different name, and every per-package
/// command scopes its glob to that new name, so nothing else reaches the old
/// one.
#[test]
fn should_prune_registries_written_before_the_scheme_joined_the_key() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    // Any registry in the current key shape proves the point; a literal keeps
    // the surviving name visible next to the stale one it is contrasted with.
    let live = pnpm_resolving_npm_resolver::mirror::get_registry_name(LIVE_REGISTRY).unwrap();
    let meta_dirs = [
        pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR,
        pnpm_resolving_npm_resolver::mirror::FULL_META_DIR,
        pnpm_resolving_npm_resolver::mirror::FULL_FILTERED_META_DIR,
    ];
    for meta_dir in meta_dirs {
        for registry_name in [live.as_str(), "registry.npmjs.org"] {
            let dir = cwd.npmrc_info.cache_dir.join(meta_dir).join(registry_name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("is-positive.jsonl"), "{}").unwrap();
        }
    }

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let pruned: Vec<&str> = stdout.lines().collect();
    let mut expected = meta_dirs
        .map(|meta_dir| format!("{meta_dir}/registry.npmjs.org"))
        .to_vec();
    expected.sort();
    assert_eq!(pruned, expected);
    for meta_dir in meta_dirs {
        let stale = cwd.npmrc_info.cache_dir.join(meta_dir).join("registry.npmjs.org");
        assert!(!stale.exists(), "expected {stale:?} to be pruned");
        let kept = cwd.npmrc_info.cache_dir
            .join(meta_dir)
            .join(&live)
            .join("is-positive.jsonl");
        assert!(kept.exists(), "expected {kept:?} to survive the prune");
    }
}

/// The deletion rests on the shape of a directory name, and the cache is shared
/// with any other pnpm on the machine, so a user has to be able to see the list
/// before committing to it.
#[test]
fn should_report_but_keep_stale_registries_on_a_dry_run() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let stale = cwd.npmrc_info.cache_dir
        .join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR)
        .join("registry.npmjs.org");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("is-positive.jsonl"), "{}").unwrap();

    let assertion = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .with_arg("--dry-run")
        .assert()
        .success();
    let output = assertion.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [format!(
            "{}/registry.npmjs.org",
            pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR
        )],
    );
    assert!(stale.join("is-positive.jsonl").exists(), "a dry run must remove nothing");
    assert!(
        stderr.contains("1 directory would be deleted"),
        "the notice must agree in number with the one directory found, got: {stderr}",
    );
}

/// Restores a directory this test sealed, on the way out of the test whether it
/// passed or panicked.
///
/// `chmod 000` does not deny uid 0, so under a root-privileged runner the
/// assertions below fail. Without this the failing assertion would unwind past
/// the restore and leave the `TempDir` a subtree it cannot remove.
#[cfg(unix)]
struct RestoreMode<'a>(&'a Path);

#[cfg(unix)]
impl Drop for RestoreMode<'_> {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;

        let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o755));
    }
}

/// A root it cannot read must not cost the user the roots it can. The failure
/// also has to name the directory: `Permission denied` on its own leaves nobody
/// anything to fix.
#[cfg(unix)]
#[test]
fn should_prune_the_readable_roots_when_another_root_cannot_be_read() {
    use std::os::unix::fs::PermissionsExt as _;

    let cwd = CommandTempCwd::init().add_mocked_registry();

    let sealed =
        cwd.npmrc_info.cache_dir.join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR);
    fs::create_dir_all(sealed.join("registry.npmjs.org")).unwrap();
    let reachable = cwd.npmrc_info.cache_dir
        .join(pnpm_resolving_npm_resolver::mirror::FULL_META_DIR)
        .join("registry.yarnpkg.com");
    fs::create_dir_all(&reachable).unwrap();
    fs::set_permissions(&sealed, fs::Permissions::from_mode(0o000)).unwrap();
    let _restore = RestoreMode(&sealed);

    let assertion = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .assert()
        .failure();
    let output = assertion.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The trailing quote of the debug-printed path pins which root is named:
    // `v11/metadata` is otherwise a prefix of the two roots that did not fail.
    let sealed_root = format!(r#"{}":"#, pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR);
    assert!(stderr.contains(&sealed_root), "failure must name the unreadable root, got: {stderr}");
    assert!(
        !stderr.contains(pnpm_resolving_npm_resolver::mirror::FULL_META_DIR),
        "the root that read cleanly must not be reported as failing, got: {stderr}",
    );
    assert!(
        stdout.contains("registry.yarnpkg.com"),
        "the readable root must still be reclaimed, got: {stdout}",
    );
    assert!(!reachable.exists(), "expected {reachable:?} to be pruned");
}

/// The other half of the same contract: a directory it cannot delete is named
/// too, and survives.
#[cfg(unix)]
#[test]
fn should_name_the_directory_it_could_not_remove() {
    use std::os::unix::fs::PermissionsExt as _;

    let cwd = CommandTempCwd::init().add_mocked_registry();

    let root =
        cwd.npmrc_info.cache_dir.join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR);
    let stale = root.join("registry.npmjs.org");
    fs::create_dir_all(&stale).unwrap();
    // Listing the root stays allowed, so the stale directory is still found;
    // unlinking it needs write permission on the root, which this denies.
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
    let _restore = RestoreMode(&root);

    let assertion = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();

    assert!(
        stderr.contains("Failed to remove metadata cache directory"),
        "failure must say what it could not do, got: {stderr}",
    );
    assert!(
        stderr.contains("registry.npmjs.org"),
        "failure must name the directory it could not remove, got: {stderr}",
    );
    assert!(stale.exists(), "a failed removal must leave {stale:?} in place");
}

/// `cacheDir` is a `pnpm-workspace.yaml` setting, so a checked-out project
/// chooses where prune deletes from. `read_dir` follows a symlinked root, which
/// would put every directory behind the link in reach of `remove_dir_all`.
#[cfg(unix)]
#[test]
fn should_refuse_to_prune_through_a_symlinked_metadata_root() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    // Outside the cache directory, and named so that prune would take it for a
    // pre-scheme leftover the moment it reached it.
    let outside = cwd.root.path().join("outside");
    let bystander = outside.join("registry.npmjs.org");
    fs::create_dir_all(&bystander).unwrap();
    fs::write(bystander.join("keep.txt"), "not pnpm's to delete").unwrap();

    let root =
        cwd.npmrc_info.cache_dir.join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();

    let assertion = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();

    assert!(
        stderr.contains("Refusing to prune"),
        "the refusal must say why the root was skipped, got: {stderr}",
    );
    assert!(
        bystander.join("keep.txt").exists(),
        "expected {bystander:?} to survive a prune through a symlinked root",
    );
}

/// A path is its own prefix, so a root linked back to the cache directory
/// satisfies a containment check written as one. Prune would then list that
/// directory's own children and take every one of them for a stale registry
/// key, `v11` and the rest of the cache included.
#[cfg(unix)]
#[test]
fn should_refuse_to_prune_a_metadata_root_linked_to_the_cache_directory() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let bystander = cwd.npmrc_info.cache_dir.join("dlx");
    fs::create_dir_all(&bystander).unwrap();
    fs::write(bystander.join("keep.txt"), "not prune's to delete").unwrap();

    let root =
        cwd.npmrc_info.cache_dir.join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&cwd.npmrc_info.cache_dir, &root).unwrap();

    let assertion = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();

    assert!(
        stderr.contains("Refusing to prune"),
        "the refusal must say why the root was skipped, got: {stderr}",
    );
    assert!(
        bystander.join("keep.txt").exists(),
        "expected {bystander:?} to survive a root linked to the cache directory",
    );
}

/// Silence is how a real prune says it found nothing, so the mode that exists to
/// answer whether there is anything to reclaim has to answer out loud.
#[test]
fn should_report_a_zero_count_on_a_dry_run_of_a_clean_cache() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let live = pnpm_resolving_npm_resolver::mirror::get_registry_name(LIVE_REGISTRY).unwrap();
    let dir = cwd.npmrc_info.cache_dir
        .join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR)
        .join(&live);
    fs::create_dir_all(&dir).unwrap();

    let assertion = cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .with_arg("--dry-run")
        .assert()
        .success();
    let output = assertion.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("0 directories would be deleted"),
        "a dry run must say so even when it finds nothing, got: {stderr}",
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "", "nothing to list on stdout");
}

/// Nothing to reclaim must be a quiet success, not an error or a stray blank
/// line, because a user runs this to find out whether there is anything there.
#[test]
fn should_prune_nothing_when_every_registry_is_readable() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    // Any registry in the current key shape proves the point; a literal keeps
    // the surviving name visible next to the stale one it is contrasted with.
    let live = pnpm_resolving_npm_resolver::mirror::get_registry_name(LIVE_REGISTRY).unwrap();
    let dir = cwd.npmrc_info.cache_dir
        .join(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR)
        .join(&live);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("is-positive.jsonl"), "{}").unwrap();

    cwd.pacquet
        .with_arg("cache")
        .with_arg("prune")
        .assert()
        .success()
        .stdout("");

    assert!(dir.join("is-positive.jsonl").exists());
}

#[test]
fn should_view_package_cache() {
    let cwd = CommandTempCwd::init().add_mocked_registry();
    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name = pnpm_resolving_npm_resolver::mirror::get_registry_name(url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();

    let package_jsonl = "{}\n{\
        \"name\":\"is-positive\",\
        \"dist-tags\":{\"latest\":\"1.0.0\"},\
        \"versions\":{\
            \"1.0.0\":{\
                \"name\":\"is-positive\",\
                \"version\":\"1.0.0\",\
                \"dist\":{\
                    \"integrity\":\"sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==\"\
                }\
            }\
        }\
    }";
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), package_jsonl).unwrap();

    let output = cwd.pacquet
        .with_args(["cache", "view", "is-positive"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let key = pnpm_resolving_npm_resolver::mirror::decode_registry_name(&registry_name);
    assert!(json.get(&key).is_some());
    let info = json.get(&key).unwrap();
    assert!(info.get("cachedVersions").is_some());
    assert!(info.get("nonCachedVersions").is_some());
    assert!(info.get("cachedAt").is_some());
}

#[test]
fn import_populates_metadata_cache() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 1,
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": { "version": "100.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write package-lock.json");

    pacquet
        .with_arg("import")
        .assert()
        .success();

    let registry_name =
        pnpm_resolving_npm_resolver::mirror::get_registry_name(mock_instance.url()).unwrap();
    let cache_metadata_dir = cache_dir
        .join("v11")
        .join("metadata")
        .join(&registry_name);

    assert!(cache_metadata_dir.exists(), "metadata cache directory must exist");
    assert!(
        cache_metadata_dir.join("@pnpm.e2e/pkg-with-1-dep.jsonl").exists(),
        "cached metadata file for @pnpm.e2e/pkg-with-1-dep must exist",
    );
    assert!(
        cache_metadata_dir.join("@pnpm.e2e/dep-of-pkg-with-1-dep.jsonl").exists(),
        "cached metadata file for transitive dependency @pnpm.e2e/dep-of-pkg-with-1-dep must exist",
    );

    drop((root, mock_instance));
}

#[test]
fn should_print_cache_path() {
    let cwd = CommandTempCwd::init().add_mocked_registry();
    let cache_dir = cwd.npmrc_info.cache_dir.clone();

    let output = cwd.pacquet
        .with_arg("cache")
        .with_arg("path")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let printed = PathBuf::from(String::from_utf8(output).unwrap().trim());
    // The path is meant to be handed to other tools, so it must be absolute
    // and free of `..` — the configured `cacheDir` is relative to the
    // workspace. Its textual form is not pinned any further: macOS resolves
    // the temporary directory to `/private/var`, exactly as `path.resolve`
    // does for the TypeScript CLI.
    assert!(printed.is_absolute(), "expected an absolute path, got {}", printed.display());
    assert!(
        !printed
            .components()
            .any(|component| component == Component::ParentDir),
        "expected a cleaned path, got {}",
        printed.display(),
    );
    fs::create_dir_all(&printed).unwrap();
    assert_eq!(fs::canonicalize(&printed).unwrap(), fs::canonicalize(&cache_dir).unwrap());
}
