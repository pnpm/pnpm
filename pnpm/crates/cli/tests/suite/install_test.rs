use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

#[test]
fn install_test() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    // Create a package.json with a test script and a dependency
    std::fs::write(
        workspace.join("package.json"),
        r#"{
            "name": "test-install-test",
            "scripts": {
                "test": "node -e \"console.log('test ran successfully');\""
            },
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0"
            }
        }"#,
    )
    .unwrap();

    let output = pacquet
        .with_args(["install-test"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("stdout:\n{stdout}");
    assert!(stdout.contains("test ran successfully"), "stdout: {stdout}");

    let hello_world_bin_dir = workspace
        .join("node_modules")
        .join("@pnpm.e2e")
        .join("hello-world-js-bin");
    println!("Checking if dependency directory exists: {}", hello_world_bin_dir.display());
    assert!(hello_world_bin_dir.exists(), "dependency not installed");
    drop((root, npmrc_info));
}

#[test]
fn install_test_failure_prevents_test() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    std::fs::write(
        workspace.join("package.json"),
        r#"{
            "name": "test-install-test",
            "scripts": {
                "test": "node -e \"console.log('test ran successfully');\""
            },
            "dependencies": {
                "does-not-exist-in-any-registry": "1.0.0"
            }
        }"#,
    )
    .unwrap();

    let output = pacquet
        .with_args(["install-test"])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("stdout:\n{stdout}");
    assert!(
        !stdout.contains("test ran successfully"),
        "test script should not run on install failure",
    );

    drop((root, npmrc_info));
}

#[test]
fn it_alias() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    std::fs::write(
        workspace.join("package.json"),
        r#"{
            "name": "test-it-alias",
            "scripts": {
                "test": "node -e \"console.log('it alias ran successfully');\""
            }
        }"#,
    )
    .unwrap();

    let output = pacquet
        .with_args(["it"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("stdout:\n{stdout}");
    assert!(stdout.contains("it alias ran successfully"), "stdout: {stdout}");

    drop((root, npmrc_info));
}

#[test]
fn recursive_install_test_no_bail_continues_after_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    std::fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - project-*\n").unwrap();
    for (name, script) in [
        ("project-1", r"require('fs').appendFileSync('../order.txt', 'first\n'); process.exit(1)"),
        ("project-2", r"require('fs').appendFileSync('../order.txt', 'second\n')"),
    ] {
        let dir = workspace.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            serde_json::json!({ "name": name, "scripts": { "test": "node test.cjs" } }).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("test.cjs"), script).unwrap();
    }

    pacquet
        .with_args(["-r", "--workspace-concurrency=1", "--no-bail", "install-test"])
        .assert()
        .code(1);

    let order = std::fs::read_to_string(workspace.join("order.txt")).unwrap();
    eprintln!("test execution order:\n{order}");
    assert_eq!(order, "first\nsecond\n");
    drop(root);
}
