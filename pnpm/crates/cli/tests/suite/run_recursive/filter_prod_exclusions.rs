use super::{
    CommandExtra, CommandTempCwd, Path, Value, append_line_script, fs, json, sorted_lines,
};
use assert_cmd::assert::OutputAssertExt;

fn setup_filter_prod_workspace(workspace: &Path, log_path: &Path) {
    let root_manifest = check_records_package(log_path, "ws-root");
    let mut app = check_records_package(log_path, "app");
    app["dependencies"] = json!({ "lib-a": "workspace:*" });
    app["devDependencies"] = json!({ "lib-dev": "workspace:*" });
    let packages = [
        ("app", app),
        ("lib-a", check_records_package(log_path, "lib-a")),
        ("lib-dev", check_records_package(log_path, "lib-dev")),
        ("unrelated", check_records_package(log_path, "unrelated")),
    ];
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), root_manifest.to_string())
        .expect("write root package.json");
    for (name, manifest) in packages {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

fn check_records_package(log_path: &Path, name: &str) -> Value {
    let log = log_path.to_str().expect("utf-8 temp path");
    json!({
        "name": name,
        "version": "1.0.0",
        "scripts": {
            "check": append_line_script(name, log),
        },
    })
}

#[test]
fn recursive_run_filter_prod_with_regular_exclusion_excludes_target_and_unrelated() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let log_path = workspace.join("ran.log");
    setup_filter_prod_workspace(&workspace, &log_path);

    pacquet
        .with_args(["--filter-prod", "app...", "--filter", "!lib-a", "run", "check"])
        .assert()
        .success();

    assert_eq!(sorted_lines(&fs::read(&log_path).expect("read ran.log")), vec!["app"]);
    drop(root);
}

#[test]
fn recursive_run_regular_inclusion_with_filter_prod_exclusion_subtracts_prod_closure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let log_path = workspace.join("ran.log");
    setup_filter_prod_workspace(&workspace, &log_path);

    pacquet
        .with_args(["--filter", "app...", "--filter-prod", "!lib-a", "run", "check"])
        .assert()
        .success();

    assert_eq!(sorted_lines(&fs::read(&log_path).expect("read ran.log")), vec!["app", "lib-dev"]);
    drop(root);
}

#[test]
fn recursive_run_leading_exclusion_reincluded_by_filter_prod_preserves_cli_order() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let log_path = workspace.join("ran.log");
    setup_filter_prod_workspace(&workspace, &log_path);

    pacquet
        .with_args(["--filter", "!{packages/*}", "--filter-prod", "app...", "run", "check"])
        .assert()
        .success();

    assert_eq!(
        sorted_lines(&fs::read(&log_path).expect("read ran.log")),
        vec!["app", "lib-a", "ws-root"],
    );
    drop(root);
}
