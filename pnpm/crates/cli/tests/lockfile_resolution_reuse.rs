//! A second non-frozen install reuses the prior lockfile's resolution
//! and transitive subtree for an unchanged dependency, instead of
//! re-resolving it from the registry.
//!
//! See `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`. pacquet avoids
//! re-resolving an unchanged tree by reading the prior lockfile's
//! recorded resolution + child refs, so a re-install with the registry
//! gone still succeeds for the unchanged subtree.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, net::TcpListener, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// A `registry=` URL on a localhost port with nothing listening, so any
/// resolution attempt against it fails fast with a connection refusal.
fn dead_registry_url() -> String {
    // Bind to an ephemeral port, read it, then drop the listener so the
    // port is (almost certainly) free again — anything that connects to
    // it gets refused.
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).expect("bind an ephemeral port to learn a free one");
    let addr = listener.local_addr().expect("read the ephemeral port");
    drop(listener);
    format!("http://127.0.0.1:{}/", addr.port())
}

#[test]
fn reuses_unchanged_subtree_without_re_resolving_from_the_registry() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    // Trust the lockfile so the post-resolution verifier doesn't fetch
    // each entry's metadata from the registry — that verification is a
    // separate concern from resolution reuse, and (now that it always runs
    // and fails closed) it would hit the dead registry regardless of
    // whether resolution was reused, masking what this test proves.
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{existing}trustLockfile: true\n"))
        .expect("append trustLockfile to pnpm-workspace.yaml");

    // `@pnpm.e2e/pkg-with-1-dep@100.0.0` depends on
    // `@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0`, so the lockfile records
    // a two-node subtree (the direct dep plus its transitive child).
    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } })
            .to_string(),
    )
    .expect("write package.json");

    // Fresh install against the live registry: warms the store and writes
    // the lockfile.
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0")
            && lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@"),
        "the fresh install must record the direct dep and its transitive child:\n{lockfile}",
    );

    // Repoint the registry at a dead port. Any re-resolution now fails.
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    // Widen the range to `^100.0.0`. The locked `100.0.0` still satisfies
    // it (so the dep is reusable), but the manifest change forces the
    // non-frozen fresh-lockfile resolution path rather than the
    // up-to-date short-circuit.
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "^100.0.0" } })
            .to_string(),
    )
    .expect("rewrite package.json with a widened range");

    // Succeeds only because the unchanged subtree is reused from the
    // lockfile — re-resolving either package would hit the dead registry.
    pacquet_at(&workspace).with_arg("install").assert().success();

    drop((root, mock_instance));
}

/// A lockfile produced via the reuse path is byte-for-byte identical to
/// one produced by resolving the same manifest entirely from scratch.
///
/// The discriminating test above proves reuse *fires*; this proves it's
/// *correct* — that reusing an unchanged subtree yields the same tree a
/// fresh resolve would, so reuse can never silently drift the resolution.
///
/// Compared **byte-for-byte**: the writer sorts every lockfile map by its
/// rendered key, so build-insertion order no longer leaks into the file. A
/// reuse build and a fresh build of the same manifest therefore emit
/// identical bytes — this is the byte-stability guarantee from
/// [#12117](https://github.com/pnpm/pnpm/issues/12117).
#[test]
fn a_reused_tree_is_structurally_identical_to_a_fresh_resolve() {
    let both = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0", "@pnpm.e2e/foo": "100.0.0" }
    })
    .to_string();

    let reused = CommandTempCwd::init().add_mocked_registry();
    let reused_manifest = reused.workspace.join("package.json");
    fs::write(
        &reused_manifest,
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } })
            .to_string(),
    )
    .expect("write the reuse scenario's initial manifest");
    pacquet_at(&reused.workspace).with_arg("install").assert().success();
    fs::write(&reused_manifest, &both).expect("add the second dep to the reuse scenario");
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&reused_manifest)
        .and_then(|file| file.set_times(std::fs::FileTimes::new().set_modified(future)))
        .expect("bump manifest mtime");
    pacquet_at(&reused.workspace).with_arg("install").assert().success();
    let reused_lockfile =
        fs::read_to_string(reused.workspace.join("pnpm-lock.yaml")).expect("read reused lockfile");

    let fresh = CommandTempCwd::init().add_mocked_registry();
    fs::write(fresh.workspace.join("package.json"), &both).expect("write the fresh manifest");
    pacquet_at(&fresh.workspace).with_arg("install").assert().success();
    let fresh_lockfile =
        fs::read_to_string(fresh.workspace.join("pnpm-lock.yaml")).expect("read fresh lockfile");

    pretty_assertions::assert_eq!(
        reused_lockfile,
        fresh_lockfile,
        "a tree built via subtree reuse must serialize byte-for-byte identically to a fresh resolve",
    );

    drop((reused, fresh));
}

/// An edge denied subtree reuse re-resolves from the registry instead of
/// reading another edge's reused resolution out of the wanted-dep cache.
///
/// The synthesized `ResolveResult` a reused node is built from carries a
/// manifest without `dependencies` (a reused node's children come from
/// the snapshot graph), so it must never satisfy a fresh-resolve cache
/// lookup: a fresh edge walks children from the manifest, and the
/// dependency-less manifest would make it record the package as a leaf.
/// When that leaf occurrence sits at a shallower depth than the healthy
/// reused occurrence it wins children ownership, so the package's
/// snapshot collapses to `{}`, its peer suffix is dropped, and its
/// dependents re-point at the bare instance
/// (`'@yarnpkg/shell@4.0.0': {}` in the original report).
///
/// Scenario, driven by the `@pnpm.e2e/reuse-chain-*` fixtures
/// (`grand → parent → target`, where `target` deps `@pnpm.e2e/abc` +
/// `@pnpm.e2e/dep-of-pkg-with-1-dep`):
///
/// * `pkg-a` deps `grand`, so its unchanged walk reuses `target` at
///   depth 2 and caches the synthesized resolution under the exact-pin
///   wanted key.
/// * `pkg-b` deps `parent` plus `dep-of-pkg-with-1-dep` directly. The
///   test bumps that direct dep; `target`'s snapshot also depends on
///   it, so pkg-b's transitive edge to `target` (depth 1) is denied
///   reuse by the changed-direct-dep gate and resolves fresh — with
///   the same wanted key pkg-a already cached.
///
/// Importers resolve in order, so pkg-a's cache entry exists when
/// pkg-b's denied edge looks up; the depth-1 occurrence out-ranks the
/// depth-2 one for children ownership, making the corruption (before
/// the fix) deterministic rather than a race.
///
/// `target`'s dep `@pnpm.e2e/abc` wants peers nothing in the subtree
/// provides, so auto-install-peers suffixes `target` — the corrupted
/// instance is then visible as a distinct bare-key `{}` snapshot rather
/// than colliding with the healthy suffixed one.
#[test]
fn an_edge_denied_reuse_keeps_the_subtree_instead_of_reading_the_synthesized_reuse_result() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'pkg-a'\n  - 'pkg-b'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir(workspace.join("pkg-a")).expect("mkdir pkg-a");
    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/reuse-chain-grand": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write pkg-a/package.json");

    let pkg_b_manifest = |dep_of_pkg_with_1_dep: &str| {
        serde_json::json!({
            "name": "pkg-b",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/reuse-chain-parent": "1.0.0",
                "@pnpm.e2e/dep-of-pkg-with-1-dep": dep_of_pkg_with_1_dep,
            },
        })
        .to_string()
    };
    fs::create_dir(workspace.join("pkg-b")).expect("mkdir pkg-b");
    let pkg_b_manifest_path = workspace.join("pkg-b/package.json");
    fs::write(&pkg_b_manifest_path, pkg_b_manifest("100.0.0")).expect("write pkg-b/package.json");

    pacquet_at(workspace).with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("'@pnpm.e2e/reuse-chain-target@1.0.0(@pnpm.e2e/peer-a@"),
        "the fresh install must suffix the target with the auto-installed peers:\n{lockfile}",
    );

    // Bump `dep-of-pkg-with-1-dep` in pkg-b only: `target`'s snapshot
    // depends on it, so pkg-b's edge to `target` is denied reuse while
    // pkg-a's (already-walked) edge reused it.
    fs::write(&pkg_b_manifest_path, pkg_b_manifest("100.1.0"))
        .expect("bump dep-of-pkg-with-1-dep in pkg-b");
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    fs::OpenOptions::new()
        .write(true)
        .open(&pkg_b_manifest_path)
        .and_then(|file| file.set_times(std::fs::FileTimes::new().set_modified(future)))
        .expect("bump manifest mtime");
    pacquet_at(workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml after bump");
    assert!(
        !lockfile.contains("'@pnpm.e2e/reuse-chain-target@1.0.0': {}"),
        "the denied edge must not record the target as an empty leaf:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/reuse-chain-target@1.0.0(@pnpm.e2e/peer-a@"),
        "the target must keep its peer-suffixed snapshot:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/abc': 1.0.0("),
        "the target's snapshot must keep its dependency on @pnpm.e2e/abc:\n{lockfile}",
    );

    drop(fixture);
}

/// Re-installing an unchanged manifest must leave `pnpm-lock.yaml`
/// byte-identical: the lockfile maps are sorted at emit time, so the
/// `importers` / `packages` / `snapshots` / dependency maps don't
/// serialize in `HashMap` iteration order and a no-op re-install can't
/// reorder the file into a spurious git diff
/// ([#12117](https://github.com/pnpm/pnpm/issues/12117)). The manifest
/// carries several dependencies so at least one map holds multiple keys,
/// giving order a chance to differ.
#[test]
fn reinstalling_an_unchanged_manifest_keeps_the_lockfile_byte_identical() {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/bar": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }
    })
    .to_string();

    let project = CommandTempCwd::init().add_mocked_registry();
    fs::write(project.workspace.join("package.json"), &manifest).expect("write manifest");

    pacquet_at(&project.workspace).with_arg("install").assert().success();
    let lockfile_path = project.workspace.join("pnpm-lock.yaml");
    let first = fs::read_to_string(&lockfile_path).expect("read lockfile after first install");

    pacquet_at(&project.workspace).with_arg("install").assert().success();
    let second = fs::read_to_string(&lockfile_path).expect("read lockfile after second install");

    pretty_assertions::assert_eq!(
        first,
        second,
        "a no-op re-install must not reorder the lockfile",
    );

    drop(project);
}
