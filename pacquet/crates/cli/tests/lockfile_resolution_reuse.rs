//! A second non-frozen install reuses the prior lockfile's resolution
//! and transitive subtree for an unchanged dependency, instead of
//! re-resolving it from the registry.
//!
//! See `pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md`. pnpm avoids
//! re-resolving an unchanged tree by reading the prior lockfile's
//! recorded resolution + child refs
//! ([`getInfoFromLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1199-L1248));
//! pacquet ports that so a re-install with the registry gone still
//! succeeds for the unchanged subtree.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, net::TcpListener, path::Path, process::Command, time::Duration};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
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
/// rendered key (matching pnpm's
/// [`sortLockfileKeys`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/sortLockfileKeys.ts)),
/// so build-insertion order no longer leaks into the file. A reuse build and
/// a fresh build of the same manifest therefore emit identical bytes — this
/// is the byte-stability guarantee from
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
    // APFS uses coarse mtime granularity; ensure the manifest write is
    // visible to the subprocess before it reads the file.
    std::thread::sleep(Duration::from_millis(100));
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
