use super::link_manifest_link_deps;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::SilentReporter;
use pacquet_testing_utils::fs::is_symlink_or_junction;
use std::fs;
use tempfile::tempdir;

fn manifest_at(dir: &std::path::Path, json: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(dir.join("package.json"), json)
}

/// `link:` specs from the in-memory manifests are materialized as
/// symlinks under the project's `node_modules/` — absolute payloads
/// as-is, relative payloads anchored at the project dir, and the
/// `link:.` self-reference pointing back at the project itself.
/// Non-`link:` specs are left for the lockfile passes.
#[test]
fn links_absolute_relative_and_self_reference_specs() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    let external = dir.path().join("external-pkg");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&external).unwrap();
    fs::create_dir_all(dir.path().join("sibling")).unwrap();

    let manifest = manifest_at(
        &project_dir,
        serde_json::json!({
            "name": "project",
            "dependencies": {
                "abs-linked": format!("link:{}", external.display()),
                "rel-linked": "link:../sibling",
                "registry-dep": "^1.0.0",
            },
            "devDependencies": {
                "project": "link:.",
            },
        }),
    );

    link_manifest_link_deps::<SilentReporter>(
        dir.path(),
        &[(project_dir.clone(), &manifest)],
        None,
        std::ffi::OsStr::new("node_modules"),
    )
    .expect("linking succeeds");

    let modules = project_dir.join("node_modules");
    assert!(is_symlink_or_junction(&modules.join("abs-linked")).unwrap());
    assert_eq!(
        fs::canonicalize(modules.join("abs-linked")).unwrap(),
        external.canonicalize().unwrap(),
    );
    assert!(is_symlink_or_junction(&modules.join("rel-linked")).unwrap());
    assert_eq!(
        fs::canonicalize(modules.join("rel-linked")).unwrap(),
        dir.path().join("sibling").canonicalize().unwrap(),
    );
    // `link:.` self-reference resolves back to the project dir.
    assert_eq!(
        fs::canonicalize(modules.join("project")).unwrap(),
        project_dir.canonicalize().unwrap(),
    );
    // Registry specs are not this pass's job.
    assert!(!modules.join("registry-dep").exists());

    drop(dir);
}

/// Re-running the pass replaces a stale symlink (v11 re-link
/// semantics) instead of failing on the existing entry.
#[test]
fn relink_replaces_stale_symlink() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    let old_target = dir.path().join("old");
    let new_target = dir.path().join("new");
    for d in [&project_dir, &old_target, &new_target] {
        fs::create_dir_all(d).unwrap();
    }
    fs::create_dir_all(project_dir.join("node_modules")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&old_target, project_dir.join("node_modules/dep")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&old_target, project_dir.join("node_modules/dep")).unwrap();

    let manifest = manifest_at(
        &project_dir,
        serde_json::json!({
            "name": "project",
            "dependencies": { "dep": format!("link:{}", new_target.display()) },
        }),
    );
    link_manifest_link_deps::<SilentReporter>(
        dir.path(),
        &[(project_dir.clone(), &manifest)],
        None,
        std::ffi::OsStr::new("node_modules"),
    )
    .expect("relink succeeds");
    assert_eq!(
        fs::canonicalize(project_dir.join("node_modules/dep")).unwrap(),
        new_target.canonicalize().unwrap(),
    );

    drop(dir);
}

/// An alias the lockfile importer already resolves is left to the
/// lockfile passes — even when the manifest spells it as a `link:` —
/// so a `dedupeDirectDeps` decision (no symlink under the importer)
/// is not undone by the manifest pass.
#[test]
fn lockfile_tracked_alias_is_skipped() {
    use pacquet_lockfile::{ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec};

    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("packages/sibling");
    let shared = dir.path().join("packages/shared");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&shared).unwrap();

    let manifest = manifest_at(
        &project_dir,
        serde_json::json!({
            "name": "sibling",
            "dependencies": { "shared": "link:../shared" },
        }),
    );

    let mut deps = ResolvedDependencyMap::new();
    deps.insert(
        "shared".parse().unwrap(),
        ResolvedDependencySpec {
            specifier: "link:../shared".to_string(),
            version: pacquet_lockfile::ImporterDepVersion::Link("../shared".to_string()),
        },
    );
    let mut importers = std::collections::HashMap::new();
    importers.insert(
        "packages/sibling".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    );

    link_manifest_link_deps::<SilentReporter>(
        dir.path(),
        &[(project_dir.clone(), &manifest)],
        Some(&importers),
        std::ffi::OsStr::new("node_modules"),
    )
    .expect("pass succeeds");
    assert!(
        !project_dir.join("node_modules/shared").exists(),
        "a lockfile-tracked link alias must be left to the lockfile passes",
    );

    drop(dir);
}

/// A dependency key that is not a valid npm package name (path
/// traversal, absolute path) is rejected before any filesystem write —
/// manifest keys are raw JSON strings and must not escape
/// `node_modules/`.
#[test]
fn traversal_alias_is_rejected_without_writes() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    let victim = dir.path().join("victim");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&victim).unwrap();

    for alias in ["../victim", "/abs", "..", r"a\b"] {
        let manifest = manifest_at(
            &project_dir,
            serde_json::json!({
                "name": "project",
                "dependencies": { alias: format!("link:{}", victim.display()) },
            }),
        );
        let result = link_manifest_link_deps::<SilentReporter>(
            dir.path(),
            &[(project_dir.clone(), &manifest)],
            None,
            std::ffi::OsStr::new("node_modules"),
        );
        assert!(
            matches!(result, Err(super::LinkManifestLinkDepsError::InvalidAlias(_))),
            "alias {alias:?} must be rejected",
        );
    }
    // Nothing was written anywhere.
    assert!(!project_dir.join("node_modules").exists());
    assert!(victim.exists() && fs::read_dir(&victim).unwrap().next().is_none());

    drop(dir);
}

/// A `modulesDir` override changes where the links land — the pass
/// must follow the configured basename instead of growing a stray
/// `node_modules/` next to the intended tree.
#[test]
fn custom_modules_dir_name_is_honored() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    let external = dir.path().join("external-pkg");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&external).unwrap();

    let manifest = manifest_at(
        &project_dir,
        serde_json::json!({
            "name": "project",
            "dependencies": { "dep": format!("link:{}", external.display()) },
        }),
    );
    link_manifest_link_deps::<SilentReporter>(
        dir.path(),
        &[(project_dir.clone(), &manifest)],
        None,
        std::ffi::OsStr::new("custom_modules"),
    )
    .expect("linking succeeds");

    assert_eq!(
        fs::canonicalize(project_dir.join("custom_modules/dep")).unwrap(),
        external.canonicalize().unwrap(),
    );
    assert!(!project_dir.join("node_modules").exists(), "no stray node_modules");

    drop(dir);
}

/// A modules-dir name that is not a single normal component (`.`,
/// `..`, absolute, separator-bearing, empty) is rejected before any
/// filesystem write — joined under a project dir it would drop
/// force-replacing symlinks outside the intended modules directory.
/// The install call site derives the name from
/// `Path::file_name()` (which never yields these), so this pins the
/// helper's own contract for other callers.
#[test]
fn non_normal_modules_dir_name_is_rejected_without_writes() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    let external = dir.path().join("external-pkg");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&external).unwrap();

    let manifest = manifest_at(
        &project_dir,
        serde_json::json!({
            "name": "project",
            "dependencies": { "dep": format!("link:{}", external.display()) },
        }),
    );

    for name in [".", "..", "", "a/b", "/abs"] {
        let result = link_manifest_link_deps::<SilentReporter>(
            dir.path(),
            &[(project_dir.clone(), &manifest)],
            None,
            std::ffi::OsStr::new(name),
        );
        assert!(
            matches!(result, Err(super::LinkManifestLinkDepsError::InvalidModulesDirName { .. })),
            "modules dir name {name:?} must be rejected",
        );
    }
    // Nothing was written: no symlink in the project dir, its parent,
    // or any would-be modules dir.
    assert!(!project_dir.join("dep").exists());
    assert!(!dir.path().join("dep").exists());
    assert!(!project_dir.join("node_modules").exists());

    drop(dir);
}

/// A manifest-linked dep that declares a `bin` gets its shim in
/// `<modules_dir>/.bin`, matching v11's `linkDirectDeps`. A linked
/// target without a `package.json` (Bit's manifest-less component
/// links) is silently skipped by the bin sweep.
#[test]
fn bins_of_manifest_linked_deps_are_linked() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    let tool = dir.path().join("tool");
    let bare = dir.path().join("bare");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&tool).unwrap();
    fs::create_dir_all(&bare).unwrap();
    fs::write(
        tool.join("package.json"),
        serde_json::json!({ "name": "tool", "version": "1.0.0", "bin": { "tool": "cli.js" } })
            .to_string(),
    )
    .unwrap();
    fs::write(tool.join("cli.js"), "#!/usr/bin/env node\n").unwrap();

    let manifest = manifest_at(
        &project_dir,
        serde_json::json!({
            "name": "project",
            "dependencies": {
                "tool": format!("link:{}", tool.display()),
                "bare": format!("link:{}", bare.display()),
            },
        }),
    );
    link_manifest_link_deps::<SilentReporter>(
        dir.path(),
        &[(project_dir.clone(), &manifest)],
        None,
        std::ffi::OsStr::new("node_modules"),
    )
    .expect("linking succeeds");

    let bin_dir = project_dir.join("node_modules/.bin");
    assert!(
        bin_dir.join("tool").exists() || bin_dir.join("tool.exe").exists(),
        "the linked dep's declared bin must land in .bin",
    );
    // The manifest-less link is placed but contributes no bins.
    assert!(project_dir.join("node_modules/bare").exists());

    drop(dir);
}
