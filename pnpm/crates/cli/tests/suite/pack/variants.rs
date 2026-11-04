//! `pnpm pack` of a package that declares `publishConfig.variants`.

use super::read_manifest_from_tarball;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::{Value, json};
use std::{fs, process::Command};

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

/// `--json` names each written archive by its integrity and size, so a
/// publish from other machines can reference it, and a dry run, which writes
/// none, names neither.
#[test]
fn pack_json_names_each_archive_by_integrity_and_size() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "example-cli",
            "version": "4.0.0",
            "publishConfig": {
                "variants": [
                    { "targets": ["any"], "files": ["index.js"] },
                    { "targets": ["linux-x64"], "files": ["native/example-cli"] },
                ],
            },
        })
        .to_string(),
    )
    .unwrap();
    fs::write(workspace.join("index.js"), "module.exports = 1\n").unwrap();
    fs::create_dir_all(workspace.join("native")).unwrap();
    fs::write(workspace.join("native/example-cli"), "binary").unwrap();

    let dry_run_command = Command::new(pacquet.get_program())
        .without_ambient_pnpm_config()
        .with_current_dir(&workspace);
    let output = pacquet
        .with_args(["pack", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let archives = printed.as_array().expect("one entry per archive");
    let tags: Vec<&Value> = archives
        .iter()
        .map(|archive| &archive["tag"])
        .collect();
    assert_eq!(tags, [&Value::Null, &json!("linux-x64")], "{printed}");
    for archive in archives {
        let bytes = fs::read(workspace.join(archive["filename"].as_str().unwrap())).unwrap();
        assert_eq!(
            archive["integrity"],
            ssri::IntegrityOpts::new()
                .algorithm(ssri::Algorithm::Sha512)
                .chain(&bytes)
                .result()
                .to_string(),
            "{archive}",
        );
        assert_eq!(archive["size"], bytes.len(), "{archive}");
    }

    let dry_run = dry_run_command
        .with_args(["pack", "--json", "--dry-run"])
        .output()
        .unwrap();
    assert!(dry_run.status.success(), "{}", String::from_utf8_lossy(&dry_run.stderr));
    let printed: Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    for archive in printed.as_array().unwrap() {
        assert!(archive.get("integrity").is_none() && archive.get("size").is_none(), "{archive}");
    }

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
