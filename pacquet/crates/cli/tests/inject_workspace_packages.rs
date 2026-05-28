//! End-to-end coverage for `injectWorkspacePackages: true` in a
//! `pnpm-workspace.yaml` monorepo.
//!
//! Ports upstream's `'inject local packages using the
//! injectWorkspacePackages setting'` at
//! [`installing/deps-installer/test/install/injectLocalPackages.ts:218`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/injectLocalPackages.ts#L218-L414).
//! The upstream test asserts the three behavioral consequences of the
//! global flag: workspace packages materialise as `file:` snapshots
//! (not `link:`) with peer-dep hash suffixes; `dependenciesMeta` on
//! the importer is **not** populated (the global flag flips the
//! resolution scheme without a per-dep opt-in); and the lockfile
//! records `settings.injectWorkspacePackages: true` so a later
//! install with the flag flipped re-resolves.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

/// Three-project workspace with `injectWorkspacePackages: true`.
/// project-1 has its own deps + a peer requirement; project-2
/// consumes project-1 via `workspace:1.0.0`; project-3 consumes
/// project-2 the same way and pulls a different `is-positive` peer
/// version. The peer-resolver produces a peer-suffixed `file:`
/// resolution for each project-1 occurrence.
///
/// Assertions (a strict subset of the upstream test's — pacquet
/// doesn't yet write `injectedDeps` into `.modules.yaml`, so the
/// modules-state side is skipped; tracked separately):
///
/// - install succeeds.
/// - `pnpm-lock.yaml` carries `settings.injectWorkspacePackages: true`.
/// - The workspace resolutions are recorded as `file:` (not `link:`)
///   inside the importer entries — that's the byte-level signal that
///   the global flag took effect.
/// - The virtual-store directory layout matches the upstream count:
///   eight slots, accounting for `is-negative`, two `is-positive`
///   versions, one `@pnpm.e2e/dep-of-pkg-with-1-dep`, its single
///   transitive registry dep, and three peer-suffixed file
///   resolutions (project-1 × {is-positive@1.0.0, is-positive@2.0.0}
///   + project-2 × is-positive@2.0.0).
#[test]
fn inject_workspace_packages_writes_file_resolutions_and_lockfile_setting() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // Flip the workspace yaml: `injectWorkspacePackages: true`,
    // `autoInstallPeers: false` (matches upstream's `testDefaults`
    // call), and the `packages:` glob covering the three sibling
    // projects.
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("injectWorkspacePackages: true\n");
    workspace_yaml.push_str("autoInstallPeers: false\n");
    workspace_yaml.push_str("packages:\n  - 'project-*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    // Workspace root: empty manifest so any installed dep is
    // attributable to the siblings below.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    fs::create_dir_all(workspace.join("project-1")).expect("mkdir project-1");
    fs::write(
        workspace.join("project-1/package.json"),
        serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "dependencies": { "is-negative": "1.0.0" },
            "devDependencies": {
                "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
            },
            "peerDependencies": { "is-positive": ">=1.0.0" },
        })
        .to_string(),
    )
    .expect("write project-1/package.json");

    fs::create_dir_all(workspace.join("project-2")).expect("mkdir project-2");
    fs::write(
        workspace.join("project-2/package.json"),
        serde_json::json!({
            "name": "project-2",
            "version": "1.0.0",
            "dependencies": { "project-1": "workspace:1.0.0" },
            "devDependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write project-2/package.json");

    fs::create_dir_all(workspace.join("project-3")).expect("mkdir project-3");
    fs::write(
        workspace.join("project-3/package.json"),
        serde_json::json!({
            "name": "project-3",
            "version": "1.0.0",
            "dependencies": { "project-2": "workspace:1.0.0" },
            "devDependencies": { "is-positive": "2.0.0" },
        })
        .to_string(),
    )
    .expect("write project-3/package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");

    // (1) `settings.injectWorkspacePackages: true` round-trips through
    // the writer. Mirrors upstream's
    // `expect(lockfile.settings.injectWorkspacePackages).toBe(true)`.
    assert!(
        lockfile.contains("injectWorkspacePackages: true"),
        "pnpm-lock.yaml missing `settings.injectWorkspacePackages: true`:\n{lockfile}",
    );

    // (2) The workspace dep resolutions are recorded as `file:`, not
    // `link:`. Slice each consumer's importer block and check the
    // recorded version. Upstream asserts the same shape via
    // `lockfile.importers['project-2'].dependencies.project-1.version`
    // and the analogous project-3 entry — pacquet's wire format is
    // YAML, so we slice the YAML.
    // The `file:` paths are rendered relative to the *lockfile root*
    // (the workspace dir), matching pnpm's
    // [`resolveFromLocalPackage`](https://github.com/pnpm/pnpm/blob/39101f5e37/resolving/npm-resolver/src/index.ts#L908-L951)
    // — not relative to each consumer project. So `project-2`'s
    // recorded dep on project-1 reads `project-1@file:project-1(...)`
    // even though project-2 lives a directory away.
    //
    // The lockfile's `importers` map is unordered upstream (the JSON
    // wire shape preserves whatever order serde emits), so pacquet's
    // `HashMap`-backed writer may interleave importer blocks in any
    // order. Parse the YAML and inspect the consumer importer's
    // recorded version directly through pacquet's own lockfile model.
    let parsed: pacquet_lockfile::Lockfile = serde_saphyr::from_str(&lockfile)
        .map_err(|err| {
            format!(
                "re-parse the lockfile we just wrote (this should never fail): {err}\n{lockfile}",
            )
        })
        .unwrap();

    let importer_version = |importer_id: &str, dep_name: &str| -> String {
        let dep_name_parsed = dep_name
            .parse::<pacquet_lockfile::PkgName>()
            .unwrap_or_else(|err| panic!("parse PkgName {dep_name:?}: {err}"));
        let importer = parsed.importers.get(importer_id).unwrap_or_else(|| {
            panic!("pnpm-lock.yaml missing `importers[{importer_id:?}]` block:\n{lockfile}",)
        });
        let deps = importer.dependencies.as_ref().unwrap_or_else(|| {
            panic!(
                "pnpm-lock.yaml `importers[{importer_id:?}]` has no `dependencies` block:\n{lockfile}",
            )
        });
        let spec = deps.get(&dep_name_parsed).unwrap_or_else(|| {
            panic!(
                "pnpm-lock.yaml `importers[{importer_id:?}].dependencies` missing {dep_name:?}:\n{lockfile}",
            )
        });
        spec.version.to_string()
    };

    let p2_dep_on_p1 = importer_version("project-2", "project-1");
    assert!(
        p2_dep_on_p1.starts_with("project-1@file:project-1"),
        "project-2 importer must record project-1 as `file:project-1(...)` (inject on), not `link:`; \
         got version={p2_dep_on_p1:?}",
    );
    assert!(
        !p2_dep_on_p1.starts_with("link:"),
        "project-2 importer must NOT record project-1 as `link:` when inject is on; \
         got version={p2_dep_on_p1:?}",
    );

    let p3_dep_on_p2 = importer_version("project-3", "project-2");
    assert!(
        p3_dep_on_p2.starts_with("project-2@file:project-2"),
        "project-3 importer must record project-2 as `file:project-2(...)` (inject on), not `link:`; \
         got version={p3_dep_on_p2:?}",
    );

    // (3) Virtual store cardinality: eight slots, matching upstream's
    // `expect(fs.readdirSync('node_modules/.pnpm')).toHaveLength(8)`.
    // The Bourne-of-truth count derives from the dependency graph:
    //   - is-negative@1.0.0
    //   - is-positive@1.0.0
    //   - is-positive@2.0.0
    //   - @pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0
    //   - @pnpm.e2e/dep-of-pkg-with-1-dep's single transitive
    //     registry dep
    //   - project-1 peer-resolved with is-positive@1.0.0
    //     (consumed via project-2)
    //   - project-1 peer-resolved with is-positive@2.0.0
    //     (consumed via project-3 through the injected project-2)
    //   - project-2 peer-resolved with is-positive@2.0.0
    //     (consumed via project-3)
    // Virtual store cardinality: seven slot dirs — matches the
    // graph snapshot upstream asserts (the same seven entries appear
    // in pacquet's `snapshots:` block above). Upstream's
    // `readdirSync('node_modules/.pnpm').toHaveLength(8)` includes
    // the sibling `lock.yaml` (recording what was materialised),
    // for which pacquet's wire shape is identical; counting all
    // entries here matches upstream's count exactly.
    let dot_pnpm = workspace.join("node_modules/.pnpm");
    let entries: Vec<_> = fs::read_dir(&dot_pnpm)
        .expect("read node_modules/.pnpm")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        entries.len(),
        8,
        "expected 8 entries under node_modules/.pnpm \
         (7 virtual-store slot dirs + the sibling `lock.yaml`), got {}. Contents:\n{entries:?}",
        entries.len(),
    );

    drop((root, mock_instance));
}
