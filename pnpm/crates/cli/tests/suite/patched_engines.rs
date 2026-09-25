//! `engineStrict` against the engines a `patchedDependencies` patch leaves.

use crate::_utils::{enable_gvs_in_workspace_yaml, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

const INCOMPATIBLE_ENGINES_PATCH: &str = "\
diff --git a/package.json b/package.json
--- a/package.json
+++ b/package.json
@@ -2,6 +2,6 @@
   \"name\": \"@pnpm.e2e/for-legacy-node\",
   \"version\": \"1.0.0\",
   \"engines\": {
-    \"node\": \"0.10\"
+    \"node\": \"99\"
   }
 }
";

const PATCHED_ENGINE_STRICT_YAML: &str = "\
engineStrict: true
patchedDependencies:
  '@pnpm.e2e/for-legacy-node@1.0.0': patches/for-legacy-node.patch
";

fn write_patch(workspace: &Path) {
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/for-legacy-node.patch"), INCOMPATIBLE_ENGINES_PATCH)
        .expect("write patch");
}

fn append_workspace_yaml(workspace: &Path, extra_yaml: &str) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).unwrap_or_default();
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(extra_yaml);
    fs::write(&workspace_yaml, yaml).expect("write workspace yaml");
}

fn write_optional_manifest(dir: &Path) {
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "optionalDependencies": { "legacy-node": "npm:@pnpm.e2e/for-legacy-node@1.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
}

fn assert_not_linked(link: &Path) {
    assert!(
        fs::symlink_metadata(link).is_err(),
        "incompatible optional package must not stay linked at {}",
        link.display(),
    );
}

#[test]
fn engine_strict_unlinks_a_skipped_optional_patch_from_a_workspace_project() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), r#"{"name":"root"}"#).expect("write root manifest");
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create project dir");
    write_optional_manifest(&app);
    write_patch(&workspace);
    append_workspace_yaml(
        &workspace,
        &format!("packages:\n  - packages/*\n{PATCHED_ENGINE_STRICT_YAML}"),
    );

    pacquet_in(&workspace)
        .with_args(["install"])
        .assert()
        .success();

    assert_not_linked(&app.join("node_modules/legacy-node"));

    drop((root, mock_instance));
}

#[test]
fn engine_strict_keeps_the_global_virtual_store_slot_of_a_skipped_optional_patch() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, store_dir, .. } = npmrc_info;

    write_optional_manifest(&workspace);
    write_patch(&workspace);
    enable_gvs_in_workspace_yaml(&workspace, PATCHED_ENGINE_STRICT_YAML);

    pacquet_in(&workspace)
        .with_args(["install"])
        .assert()
        .success();

    assert_not_linked(&workspace.join("node_modules/legacy-node"));
    let version_dir = store_dir.join(STORE_VERSION).join("links/@pnpm.e2e/for-legacy-node/1.0.0");
    let slots = fs::read_dir(&version_dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", version_dir.display()))
        .count();
    assert_eq!(slots, 1, "the shared slot must stay in the global virtual store");

    drop((root, mock_instance));
}
