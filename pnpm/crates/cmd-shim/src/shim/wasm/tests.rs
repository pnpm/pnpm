use super::{ScriptRuntime, generate_wasm_shim, json};
use std::{fs, process::Command};

#[test]
fn javascript_launcher_preserves_arguments_and_relocates() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("root ' dollar$");
    let bin = root.join(".bin");
    fs::create_dir_all(&bin).unwrap();
    let target = root.join("tool.cjs");
    fs::write(
        &target,
        "console.log(JSON.stringify([process.argv.slice(2), process.env.NODE_PATH]))",
    )
    .unwrap();
    let shim = bin.join("tool");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: " --no-warnings".into() };
    fs::write(
        &shim,
        generate_wasm_shim(
            &target,
            &shim,
            Some(&runtime),
            &[root.to_string_lossy().into_owned()],
            Some(&root),
        ),
    )
    .unwrap();
    let moved = temporary.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    let moved = dunce::canonicalize(moved).unwrap();
    let output = Command::new("node")
        .arg(moved.join(".bin/tool"))
        .args(["a b", "$(false)", "'quoted'"])
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(actual, json!([["a b", "$(false)", "'quoted'"], moved.to_str().unwrap()]));
}

#[test]
fn launcher_relocatability_includes_node_path_entries() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let bin = root.join(".bin");
    fs::create_dir(&bin).unwrap();
    let target = root.join("tool.js");
    fs::write(&target, "").unwrap();
    let shim = bin.join("tool");
    let internal = generate_wasm_shim(
        &target,
        &shim,
        None,
        &[root.to_string_lossy().into_owned()],
        Some(root),
    );
    assert!(super::super::relocatable::is_relocatable_shim(&internal, &bin, root));
    let external = generate_wasm_shim(&target, &shim, None, &["/outside".into()], Some(root));
    assert!(!super::super::relocatable::is_relocatable_shim(&external, &bin, root));
}

#[test]
fn malformed_runtime_arguments_fail_without_executing_the_target() {
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("target.cjs");
    fs::write(&target, "throw new Error('target executed')").unwrap();
    let shim = temporary.path().join("tool");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: " 'unterminated".into() };
    fs::write(&shim, generate_wasm_shim(&target, &shim, Some(&runtime), &[], None)).unwrap();
    let output = Command::new("node")
        .arg(&shim)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("target executed"));
}
