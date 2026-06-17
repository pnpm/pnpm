//! `dedupeDirectDeps` workspace coverage for `pacquet install`.
//!
//! Ports pnpm's
//! [`installing/deps-installer/test/install/dedupeDirectDeps.ts`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/dedupeDirectDeps.ts):
//! when the workspace root provides the same `(alias → resolution)`
//! a non-root project depends on, the dep must not appear under
//! that project's `node_modules/`, and a project whose direct deps
//! are entirely deduped must not have a `node_modules/` created at
//! all.
//!
//! `dedupeDirectDeps` is **off by default** (pnpm's config-reader
//! default at
//! [`config/reader/src/index.ts:139`](https://github.com/pnpm/pnpm/blob/a23956e3ab/config/reader/src/index.ts#L139)),
//! so the dedupe-on tests below set `dedupeDirectDeps: true` in the
//! workspace yaml explicitly, mirroring upstream's
//! `testDefaults({ ..., dedupeDirectDeps: true })`. The default-off
//! behavior — every importer keeps its own per-project symlink — is
//! covered by [`dedupe_off_by_default_keeps_shared_workspace_link`].

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fs, path::Path, process::Command};

/// With `dedupeDirectDeps: true`, a sibling project whose only
/// direct dep is also a direct dep of the workspace root must not
/// get a `node_modules/` of its own.
#[test]
fn dedupes_direct_deps_against_workspace_root() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: true\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    pacquet.with_arg("install").assert().success();

    let root_dep = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    let root_dep_linked = is_symlink_or_junction(&root_dep).expect("query root symlink");
    eprintln!("root_dep={root_dep:?} linked={root_dep_linked}");
    assert!(root_dep_linked, "root node_modules direct-dep symlink missing");

    // The deduped sibling has no node_modules at all — pnpm's
    // `linkDirectDepsAndDedupe` ends with `rimraf(project.modulesDir)`
    // when every dep was deduped. Pacquet achieves the same effect
    // by never creating the directory in the first place.
    let dup_modules = workspace.join("packages/dup/node_modules");
    let dup_modules_exists = dup_modules.exists();
    eprintln!("dup_modules={dup_modules:?} exists={dup_modules_exists}");
    assert!(
        !dup_modules_exists,
        "packages/dup/node_modules should not exist when every direct dep is deduped against root",
    );

    drop((root, mock_instance));
}

/// `dedupeDirectDeps: false` opts out — every sibling gets its
/// own per-project symlink even when the workspace root already
/// resolves the same alias to the same target.
#[test]
fn dedupe_direct_deps_disabled_keeps_per_project_symlinks() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: false\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    pacquet.with_arg("install").assert().success();

    let root_dep = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    let root_dep_linked = is_symlink_or_junction(&root_dep).expect("query root symlink");
    eprintln!("root_dep={root_dep:?} linked={root_dep_linked}");
    assert!(root_dep_linked, "root node_modules direct-dep symlink missing");
    let sibling_dep = workspace.join("packages/dup/node_modules/@pnpm.e2e/hello-world-js-bin");
    let sibling_dep_linked = is_symlink_or_junction(&sibling_dep).expect("query sibling symlink");
    eprintln!("sibling_dep={sibling_dep:?} linked={sibling_dep_linked}");
    assert!(
        sibling_dep_linked,
        "sibling direct-dep symlink should be kept when dedupeDirectDeps: false",
    );

    drop((root, mock_instance));
}

/// A frozen-lockfile install (the headless path) dedupes too.
/// Mirrors pnpm's second `mutateModules(... frozenLockfile: true)`
/// call in
/// [`dedupeDirectDeps.ts:107`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/dedupeDirectDeps.ts#L107)
/// which asserts the same on-disk shape after running through the
/// `install_frozen_lockfile` codepath.
#[test]
fn dedupes_direct_deps_with_frozen_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: true\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    // First install seeds the lockfile and node_modules.
    pacquet.with_arg("install").assert().success();
    let dup_modules = workspace.join("packages/dup/node_modules");
    let dup_modules_exists_after_seed = dup_modules.exists();
    eprintln!("after seed: dup_modules={dup_modules:?} exists={dup_modules_exists_after_seed}");
    assert!(
        !dup_modules_exists_after_seed,
        "first install should already have skipped packages/dup/node_modules creation",
    );

    // Tear down node_modules so the frozen-lockfile install is a
    // pure replay (pnpm's test does the same via `rimrafSync`).
    fs_remove_dir_all(&workspace.join("node_modules"));
    fs_remove_dir_all(&dup_modules);

    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();

    let root_dep = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    let root_dep_linked =
        is_symlink_or_junction(&root_dep).expect("query root symlink after frozen install");
    eprintln!("root_dep={root_dep:?} linked={root_dep_linked}");
    assert!(root_dep_linked, "frozen-lockfile install should re-link the root's direct dep");
    let dup_modules_exists_after_frozen = dup_modules.exists();
    eprintln!("after frozen: dup_modules={dup_modules:?} exists={dup_modules_exists_after_frozen}");
    assert!(
        !dup_modules_exists_after_frozen,
        "frozen-lockfile install should keep packages/dup/node_modules absent",
    );

    drop((root, mock_instance));
}

/// Regression for the v11.5.1 release failure
/// ([.github#214](https://github.com/pnpm/pnpm/actions/runs/26801861393)):
/// `dedupeDirectDeps` defaults to **off**, so a workspace package that
/// both the root and a non-root importer depend on must stay symlinked
/// under the non-root importer's own `node_modules/`.
///
/// The release installs through the frozen-lockfile path and then runs
/// `pnpm publish` on `@pnpm/exe`, which rewrites the `workspace:*`
/// devDependency on `@pnpm/jest-config` by reading
/// `exe/node_modules/@pnpm/jest-config/package.json`. The root also
/// depends on `@pnpm/jest-config`; when pacquet defaulted
/// `dedupeDirectDeps` to `true` it dropped the per-importer symlink, so
/// publish failed with `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL`.
/// This fixture reproduces that shape: shared `link:` workspace dep,
/// non-root importer, frozen-lockfile replay.
#[test]
fn dedupe_off_by_default_keeps_shared_workspace_link() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // Root depends on the shared workspace package, exactly like the
    // monorepo root depends on `@pnpm/jest-config`.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@scope/shared": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    // No `dedupeDirectDeps` key — exercise the default.
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/shared")).expect("mkdir packages/shared");
    fs::write(
        workspace.join("packages/shared/package.json"),
        serde_json::json!({ "name": "@scope/shared", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/shared/package.json");

    // Non-root importer that also depends on the shared package — the
    // `@pnpm/exe` analogue.
    fs::create_dir_all(workspace.join("packages/app")).expect("mkdir packages/app");
    fs::write(
        workspace.join("packages/app/package.json"),
        serde_json::json!({
            "name": "@scope/app",
            "version": "1.0.0",
            "dependencies": { "@scope/shared": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write packages/app/package.json");

    // A pnpm-written lockfile (importer-relative `link:` targets), the
    // same shape the release checks out before installing — so this
    // test isolates the dedupe behavior rather than pacquet's own
    // lockfile-writing path.
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        r#"lockfileVersion: "9.0"
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    specifiers:
      "@scope/shared": workspace:*
    dependencies:
      "@scope/shared":
        specifier: workspace:*
        version: link:packages/shared
  packages/app:
    specifiers:
      "@scope/shared": workspace:*
    dependencies:
      "@scope/shared":
        specifier: workspace:*
        version: link:../shared
  packages/shared: {}
"#,
    )
    .expect("write pnpm-lock.yaml");

    // Replay the lockfile through the frozen-lockfile path — the
    // codepath the release workflow runs.
    pacquet.with_arg("install").with_arg("--frozen-lockfile").assert().success();

    // The non-root importer must keep its own symlink, and it must
    // resolve to the shared package's manifest — that read is what
    // `pnpm publish`'s `workspace:*` rewrite performs.
    let app_link = workspace.join("packages/app/node_modules/@scope/shared");
    let app_link_linked = is_symlink_or_junction(&app_link).expect("query app symlink");
    eprintln!("app_link={app_link:?} linked={app_link_linked}");
    assert!(
        app_link_linked,
        "shared workspace dep must stay symlinked under packages/app/node_modules when \
         dedupeDirectDeps is at its default (off)",
    );
    let app_manifest = app_link.join("package.json");
    assert!(
        app_manifest.exists(),
        "packages/app/node_modules/@scope/shared must resolve to the package manifest at \
         {app_manifest:?} so `workspace:*` publish resolution succeeds",
    );

    drop((root, mock_instance));
}

fn fs_remove_dir_all(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {path:?}: {error}"),
    }
}

/// Build a fresh `pacquet` `Command` rooted at `workspace`. Used to
/// drive a second invocation in the same workspace because
/// [`assert_cmd::Command::assert`] consumes the wrapped command.
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// Partial dedupe: a sibling with one shared dep and one unique
/// dep keeps the unique dep symlinked under its `node_modules/`
/// while the shared dep is omitted.
#[test]
fn dedupes_only_overlapping_direct_deps() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: true\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/mixed")).expect("mkdir packages/mixed");
    fs::write(
        workspace.join("packages/mixed/package.json"),
        serde_json::json!({
            "name": "@scope/mixed",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0",
                "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write packages/mixed/package.json");

    pacquet.with_arg("install").assert().success();

    let mixed_modules = workspace.join("packages/mixed/node_modules");
    let shared = mixed_modules.join("@pnpm.e2e/hello-world-js-bin");
    let shared_exists = shared.exists();
    eprintln!("shared={shared:?} exists={shared_exists}");
    assert!(
        !shared_exists,
        "shared direct-dep should be deduped against root, but found {shared:?}",
    );
    let unique = mixed_modules.join("@pnpm.e2e/hello-world-js-bin-parent");
    let unique_linked = is_symlink_or_junction(&unique).expect("query unique symlink");
    eprintln!("unique={unique:?} linked={unique_linked}");
    assert!(unique_linked, "unique direct-dep symlink missing under packages/mixed/node_modules");

    drop((root, mock_instance));
}

/// Two `link:` deps that resolve to the same physical directory via
/// different relative paths must still dedupe. Pnpm's dedupe runs
/// `path.relative` on stored symlink targets — which Node normalises
/// through `path.resolve` — so `<workspace>/packages/shared` and
/// `<workspace>/packages/sibling/../shared` compare equal. Pacquet's
/// dedupe map uses lexical equality of `PathBuf`s, which would miss
/// this case unless the target paths are normalised first; this test
/// pins that normalisation.
#[test]
fn dedupes_link_deps_resolving_to_the_same_dir_via_different_segments() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "shared": "link:packages/shared" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: true\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/shared")).expect("mkdir packages/shared");
    fs::write(
        workspace.join("packages/shared/package.json"),
        serde_json::json!({ "name": "shared", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/shared/package.json");

    // Sibling reaches `packages/shared` via `../shared` — same
    // physical target as root's `packages/shared` after lexical
    // normalisation.
    fs::create_dir_all(workspace.join("packages/sibling")).expect("mkdir packages/sibling");
    fs::write(
        workspace.join("packages/sibling/package.json"),
        serde_json::json!({
            "name": "sibling",
            "version": "1.0.0",
            "dependencies": { "shared": "link:../shared" },
        })
        .to_string(),
    )
    .expect("write packages/sibling/package.json");

    pacquet.with_arg("install").assert().success();

    let root_link = workspace.join("node_modules/shared");
    let root_link_linked = is_symlink_or_junction(&root_link).expect("query root link");
    eprintln!("root_link={root_link:?} linked={root_link_linked}");
    assert!(root_link_linked, "root should have its link dep symlinked");
    // Sibling's only direct dep was a `link:../shared` resolving to
    // the same dir root already linked at the alias `shared`; the
    // sibling should be deduped and have no `node_modules/` at all.
    let sibling_modules = workspace.join("packages/sibling/node_modules");
    let sibling_modules_exists = sibling_modules.exists();
    eprintln!("sibling_modules={sibling_modules:?} exists={sibling_modules_exists}");
    assert!(
        !sibling_modules_exists,
        "packages/sibling/node_modules should not exist: link:../shared deduped against root's link:packages/shared",
    );

    drop((root, mock_instance));
}

/// Mirrors pnpm's [`'dedupe direct dependencies after public hoisting'`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/dedupeDirectDeps.ts#L113):
/// a transitive of the root that gets publicly hoisted into root's
/// `node_modules/` should dedupe a non-root importer's *direct* dep
/// with the same alias.
///
/// Asserts the dedupe behavior on the frozen-lockfile path: pacquet
/// pre-computes the hoist plan before the symlink phase so the dedupe
/// map folds in publicly-hoisted aliases. The seed install (no public
/// hoist pattern) writes the lockfile; the frozen install then runs
/// with the pattern set, hoists `dep-of-pkg-with-1-dep` to root, and
/// the dedupe pass skips re-creating the project-2 symlink for it.
#[test]
fn dedupes_direct_dep_against_publicly_hoisted_root_dep() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    let base_yaml = workspace_yaml.clone();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: true\n");
    fs::write(&workspace_yaml_path, &workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    // Seed install (no publicHoistPattern → no hoist, no dedupe
    // surprises here). The point is to produce the lockfile pacquet's
    // frozen-install path will replay.
    pacquet.with_arg("install").assert().success();

    // Flip on publicHoistPattern and clear node_modules so the
    // frozen-install path is a pure replay.
    let mut workspace_yaml = base_yaml;
    workspace_yaml.push_str(
        "packages:\n  - 'packages/*'\ndedupeDirectDeps: true\npublicHoistPattern:\n  - '@pnpm.e2e/dep-of-pkg-with-1-dep'\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    fs_remove_dir_all(&workspace.join("node_modules"));
    fs_remove_dir_all(&workspace.join("packages/dup/node_modules"));

    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();

    let root_direct = workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep");
    let root_direct_linked = is_symlink_or_junction(&root_direct).expect("query root direct dep");
    eprintln!("root_direct={root_direct:?} linked={root_direct_linked}");
    assert!(root_direct_linked, "root should still have its direct dep symlinked");
    let root_hoisted = workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep");
    let root_hoisted_linked =
        is_symlink_or_junction(&root_hoisted).expect("query root public-hoisted dep");
    eprintln!("root_hoisted={root_hoisted:?} linked={root_hoisted_linked}");
    assert!(
        root_hoisted_linked,
        "publicHoistPattern should land the transitive at root/node_modules",
    );
    // Sibling has no node_modules because its only direct dep was
    // deduped against root's public-hoisted entry.
    let dup_modules = workspace.join("packages/dup/node_modules");
    let dup_modules_exists = dup_modules.exists();
    eprintln!("dup_modules={dup_modules:?} exists={dup_modules_exists}");
    assert!(
        !dup_modules_exists,
        "packages/dup/node_modules should not exist: dep-of-pkg-with-1-dep deduped against publicly-hoisted root entry",
    );

    drop((root, mock_instance));
}

/// Mirrors pnpm's [`'shamefully-hoist + dedupe-direct-deps=true'`](https://github.com/pnpm/pnpm/blob/39101f5e37/pnpm/test/install/hoist.ts#L77):
/// with `publicHoistPattern: ['*']` (the explicit form of
/// `shamefullyHoist: true` — pacquet doesn't bridge the legacy flag to
/// the pattern, see [`hoist::shamefully_hoist_legacy_publicly_hoists_everything`]),
/// every transitive lands at the workspace root's `node_modules/`. A
/// non-root importer's direct dep that also lands at root via hoist
/// gets deduped from the importer.
///
/// Runs through pacquet's fresh-install path (single `pacquet install`,
/// no `--frozen-lockfile`), exercising the hoist pass that fresh
/// install now runs end-to-end.
#[test]
fn dedupe_under_shamefully_hoist() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(
        "packages:\n  - 'packages/*'\n\
         dedupeDirectDeps: true\n\
         shamefullyHoist: true\n\
         hoistPattern: []\n\
         publicHoistPattern:\n  - '*'\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/project")).expect("mkdir packages/project");
    fs::write(
        workspace.join("packages/project/package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foobar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/project/package.json");

    pacquet.with_arg("install").assert().success();

    for alias in [
        "@pnpm.e2e/pkg-with-1-dep",
        "@pnpm.e2e/dep-of-pkg-with-1-dep",
        "@pnpm.e2e/foobar",
        "@pnpm.e2e/foo",
    ] {
        let entry = workspace.join("node_modules").join(alias);
        let entry_linked = is_symlink_or_junction(&entry).expect("query root entry");
        eprintln!("entry={entry:?} linked={entry_linked}");
        assert!(entry_linked, "expected root/node_modules/{alias} to be a symlink");
    }

    // Project has neither its direct dep `foobar` (deduped against the
    // shamefully-hoisted root entry) nor its transitive `foo` (which
    // never reaches the project's `node_modules/` to begin with —
    // transitives only materialize via hoist).
    let project_modules = workspace.join("packages/project/node_modules");
    let project_foobar = project_modules.join("@pnpm.e2e/foobar");
    let project_foobar_exists = project_foobar.exists();
    eprintln!("project_foobar={project_foobar:?} exists={project_foobar_exists}");
    assert!(
        !project_foobar_exists,
        "project's foobar should be deduped against the shamefully-hoisted root entry",
    );
    let project_foo = project_modules.join("@pnpm.e2e/foo");
    let project_foo_exists = project_foo.exists();
    eprintln!("project_foo={project_foo:?} exists={project_foo_exists}");
    assert!(
        !project_foo_exists,
        "transitive `foo` should only appear at root via hoist, not under project",
    );

    drop((root, mock_instance));
}
