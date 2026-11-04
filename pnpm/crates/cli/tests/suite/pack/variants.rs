//! `pnpm pack` of a package that declares `publishConfig.variants`.

use super::read_manifest_from_tarball;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::fs;

#[test]
fn packs_every_archive_or_the_one_a_target_names() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "example-cli",
            "version": "4.0.0",
            "bin": { "example-cli": "bin/example-cli.js" },
            "publishConfig": {
                "variants": [
                    { "targets": ["any"], "files": ["bin/**"] },
                    {
                        "targets": ["linux-x64-musl", "linux-x64"],
                        "files": ["native/linux-x64/example-cli"],
                        "manifest": { "bin": { "example-cli": "native/linux-x64/example-cli" } },
                    },
                ],
            },
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(workspace.join("bin")).unwrap();
    fs::write(workspace.join("bin/example-cli.js"), "#!/usr/bin/env node\n").unwrap();
    fs::create_dir_all(workspace.join("native/linux-x64")).unwrap();
    fs::write(workspace.join("native/linux-x64/example-cli"), "binary").unwrap();

    let output = pacquet
        .with_args(["pack", "--variant", "linux-x64", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["tag"], "linux-x64+linux-x64-musl", "{printed}");
    assert_eq!(printed["targets"], json!(["linux-x64-musl", "linux-x64"]));
    let variant = read_manifest_from_tarball(&workspace.join(
        "example-cli-4.0.0~linux-x64+linux-x64-musl.tgz",
    ));
    assert_eq!(variant["variant"], json!({ "platforms": ["linux-x64", "linux-x64-musl"] }));
    assert_eq!(variant["bin"]["example-cli"], "native/linux-x64/example-cli");
    assert!(!workspace.join("example-cli-4.0.0.tgz").exists(), "only the named archive");

    drop(root);
}

/// A target names an entry of one package's declaration, so a recursive
/// pack does not take one.
#[test]
fn a_recursive_pack_takes_no_variant() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let output = pacquet
        .with_args(["-r", "pack", "--variant", "linux-x64"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PACK_VARIANT_ALONE"), "{stderr}");
    drop(root);
}
