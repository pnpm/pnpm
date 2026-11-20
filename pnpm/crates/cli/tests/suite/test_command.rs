use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell command")]
#[test]
fn test_runs_declared_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("tested.txt");
    let manifest = json!({
        "name": "test-command",
        "version": "0.0.0",
        "scripts": {
            "t": format!(r#"printf ran_t > "{}""#, marker.display()),
            "tst": format!(r#"printf ran_tst > "{}""#, marker.display()),
            "test": format!(r#"printf ran_test > "{}""#, marker.display()),
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet
        .with_arg("test")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(marker).expect("read marker"), "ran_test");

    drop(root);
}

#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell command")]
#[test]
fn t_alias_runs_declared_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("tested-t.txt");
    let manifest = json!({
        "name": "test-command",
        "version": "0.0.0",
        "scripts": {
            "t": format!(r#"printf ran_t > "{}""#, marker.display()),
            "tst": format!(r#"printf ran_tst > "{}""#, marker.display()),
            "test": format!(r#"printf ran_test > "{}""#, marker.display()),
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet.with_arg("t").assert().success();

    assert_eq!(fs::read_to_string(marker).expect("read marker"), "ran_test");

    drop(root);
}

#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell command")]
#[test]
fn tst_alias_runs_declared_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("tested-tst.txt");
    let manifest = json!({
        "name": "test-command",
        "version": "0.0.0",
        "scripts": {
            "t": format!(r#"printf ran_t > "{}""#, marker.display()),
            "tst": format!(r#"printf ran_tst > "{}""#, marker.display()),
            "test": format!(r#"printf ran_test > "{}""#, marker.display()),
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet
        .with_arg("tst")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(marker).expect("read marker"), "ran_test");

    drop(root);
}

#[test]
fn test_fails_when_test_script_is_missing() {
    for command in ["test", "t", "tst"] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let manifest = json!({
            "name": "test-command",
            "version": "0.0.0",
        });
        fs::write(workspace.join("package.json"), manifest.to_string())
            .expect("write package.json");

        let output = pacquet
            .with_arg(command)
            .output()
            .expect("spawn pnpm test");
        assert!(!output.status.success(), "`pnpm {command}` must fail when scripts.test is absent");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("ERR_PNPM_NO_SCRIPT"),
            "`pnpm {command}` should report ERR_PNPM_NO_SCRIPT:\n{stderr}",
        );

        drop(root);
    }
}

#[test]
fn test_succeeds_with_if_present_when_test_script_is_missing() {
    for args in [
        &["--if-present", "test"][..],
        &["--if-present", "t"],
        &["--if-present", "tst"],
        &["run", "--if-present", "test"],
    ] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let manifest = json!({
            "name": "test-command",
            "version": "0.0.0",
        });
        fs::write(workspace.join("package.json"), manifest.to_string())
            .expect("write package.json");

        pacquet
            .with_args(args)
            .assert()
            .success();

        drop(root);
    }
}

fn write_two_package_workspace(workspace: &std::path::Path) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let pkg_a = workspace.join("packages/a");
    let pkg_b = workspace.join("packages/b");
    fs::create_dir_all(&pkg_a).expect("create packages/a");
    fs::create_dir_all(&pkg_b).expect("create packages/b");
    fs::write(
        pkg_a.join("package.json"),
        json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "scripts": {
                "test": r#"node -e "require('fs').writeFileSync('tested.txt', 'ok')""#,
            },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");
    fs::write(
        pkg_b.join("package.json"),
        json!({ "name": "pkg-b", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/b/package.json");
}

#[test]
fn recursive_test_skips_packages_without_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_two_package_workspace(&workspace);

    pacquet
        .with_args(["-r", "test"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("packages/a/tested.txt")).expect("read marker"),
        "ok",
    );

    drop(root);
}

#[test]
fn filtered_test_succeeds_when_selected_package_has_no_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_two_package_workspace(&workspace);

    pacquet
        .with_args(["--filter", "pkg-b", "test"])
        .assert()
        .success();

    drop(root);
}
