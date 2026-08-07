//! Convergence overrides (`"pkg@": "<exact version>"`) end to end: the
//! override rewrites only the dependency edges its version satisfies,
//! and a full resolution warns when every declared range admits a
//! version newer than the override's value.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

/// Append an `overrides` block to the `pnpm-workspace.yaml` the mocked
/// registry already wrote (it carries `storeDir`/`cacheDir`, so the
/// tests must extend it rather than overwrite it).
fn add_overrides(workspace: &Path, block: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(block);
    fs::write(&path, yaml).expect("update pnpm-workspace.yaml");
}

fn write_manifest(workspace: &Path, dep_spec: &str) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "convergence-fixture",
            "version": "1.0.0",
            "dependencies": { DEP: dep_spec },
        })
        .to_string(),
    )
    .expect("write package.json");
}

#[test]
fn install_applies_convergence_override_and_warns_when_stale() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "^100.0.0");
    // The fixture registry serves 100.0.0, 100.1.0, and 101.0.0, so the
    // pinned 100.0.0 is stale: 100.1.0 also satisfies ^100.0.0.
    add_overrides(&workspace, &format!("overrides:\n  \"{DEP}@\": 100.0.0\n"));

    let output = pacquet.with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}\n");

    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0").exists(),
        "the convergence override should pin the satisfying edge to exactly 100.0.0",
    );
    assert!(stdout.contains(&format!(
        "The convergence override \"{DEP}@\": \"100.0.0\" is stale: \
         every declared range of {DEP} also admits 100.1.0. \
         Change the override's value to 100.1.0 in pnpm-workspace.yaml, \
         or remove the override and run \"pnpm dedupe\"."
    )));

    drop((root, mock_instance));
}

#[test]
fn install_stays_silent_when_the_convergence_override_is_not_stale() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "^100.0.0");
    add_overrides(&workspace, &format!("overrides:\n  \"{DEP}@\": 100.1.0\n"));

    let output = pacquet.with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}\n");

    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0").exists(),
        "the convergence override should rewrite the edge to 100.1.0",
    );
    assert!(
        !stdout.contains("is stale"),
        "100.1.0 is already the best version every declared range admits",
    );

    drop((root, mock_instance));
}

#[test]
fn install_leaves_incompatible_edges_on_their_own_resolution() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // 101.0.0 does not satisfy ^100.0.0, so the edge keeps its own
    // resolution (the highest of ^100.0.0) and no staleness warning
    // fires — 101.0.0 is outside every declared range.
    write_manifest(&workspace, "^100.0.0");
    add_overrides(&workspace, &format!("overrides:\n  \"{DEP}@\": 101.0.0\n"));

    let output = pacquet.with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}\n");

    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0").exists(),
        "an incompatible convergence override must leave the edge untouched",
    );
    assert!(!stdout.contains("is stale"));

    drop((root, mock_instance));
}
