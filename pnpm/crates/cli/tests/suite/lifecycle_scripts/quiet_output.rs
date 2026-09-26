use super::{
    _utils::{ndjson_records, pacquet_in},
    workspace_yaml::{allow_builds, append_workspace_yaml_key, set_strict_dep_builds},
};
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Output};

const FAILURE: &str = "@pnpm.e2e/failing-postinstall";
const SUCCESS: &str = "@pnpm.e2e/install-script-example";

fn install(workspace: &Path, level: &str, frozen: bool) -> Output {
    pacquet_in(workspace)
        .args(["install", "--loglevel", level])
        .arg(if frozen { "--frozen-lockfile" } else { "--no-frozen-lockfile" })
        .output()
        .expect("run pnpm install")
}

fn output_text(output: &Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("status: {}\n{text}", output.status);
    text
}

fn clear_modules(workspace: &Path) {
    let modules = workspace.join("node_modules");
    if modules.exists() {
        fs::remove_dir_all(modules).expect("remove fixture node_modules");
    }
}

#[test]
fn quiet_root_failures_keep_all_stdout_and_stderr_on_resolved_and_frozen_paths() {
    for level in ["warn", "error"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({
                "dependencies": {"is-positive": "1.0.0"},
                "scripts": {"postinstall": "node diagnostics.cjs"},
            })
            .to_string(),
        )
        .unwrap();
        fs::write(fixture.workspace.join("diagnostics.cjs"),
            "for (let i = 0; i < 14; i++) { console.log('root-stdout-' + i); console.error('root-stderr-' + i) }; process.exit(1)").unwrap();
        for frozen in [false, true] {
            let output = install(&fixture.workspace, level, frozen);
            let text = output_text(&output);
            assert_eq!(output.status.code(), Some(1));
            assert!(text.contains("postinstall$ node diagnostics.cjs"));
            for index in 0..14 {
                assert!(text.contains(&format!("root-stdout-{index}")));
                assert!(text.contains(&format!("root-stderr-{index}")));
            }
            assert!(text.contains("Failed"));
            assert!(!text.contains("Progress:"));
        }
    }
}

#[test]
fn quiet_dependency_failure_and_optional_status_match_on_both_install_paths() {
    for (level, optional) in [("warn", false), ("warn", true), ("error", false), ("error", true)] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        let key = if optional { "optionalDependencies" } else { "dependencies" };
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({ key: { FAILURE: "1.0.0" } }).to_string(),
        )
        .unwrap();
        append_workspace_yaml_key(&fixture.workspace, "optimisticRepeatInstall", false);
        allow_builds(&fixture.workspace, &[(FAILURE, true)]);
        for frozen in [false, true] {
            if frozen && !optional {
                let output = pacquet_in(&fixture.workspace)
                    .args(["install", "--lockfile-only", "--ignore-scripts"])
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{}", output_text(&output));
            }
            clear_modules(&fixture.workspace);
            let output = install(&fixture.workspace, level, frozen);
            assert_dependency_failure(&output, level, optional);
        }
    }
}

fn assert_dependency_failure(output: &Output, level: &str, optional: bool) {
    let text = output_text(output);
    assert_eq!(output.status.success(), optional);
    let visible = !optional || level == "warn";
    assert_eq!(text.contains("postinstall: hello"), visible);
    assert_eq!(text.contains("postinstall: world"), visible);
    assert_eq!(text.contains("postinstall$ echo hello && echo world && exit 1"), visible);
    assert_eq!(text.contains("skipped as optional"), optional && visible);
}

#[test]
fn quiet_successful_scripts_still_build_on_both_install_paths() {
    for level in ["warn", "error"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({
                "dependencies": {SUCCESS: "1.0.0"},
                "scripts": {"postinstall": "node success.cjs"},
            })
            .to_string(),
        )
        .unwrap();
        fs::write(fixture.workspace.join("success.cjs"),
            "require('fs').writeFileSync('root-built','yes'); console.log('successful-root-output')").unwrap();
        append_workspace_yaml_key(&fixture.workspace, "optimisticRepeatInstall", false);
        allow_builds(&fixture.workspace, &[(SUCCESS, true)]);
        for frozen in [false, true] {
            clear_modules(&fixture.workspace);
            let marker = fixture.workspace.join("root-built");
            if marker.exists() {
                fs::remove_file(&marker).unwrap();
            }
            let output = install(&fixture.workspace, level, frozen);
            let text = output_text(&output);
            assert!(output.status.success());
            assert_eq!(fs::read_to_string(marker).unwrap(), "yes");
            assert!(
                fixture.workspace
                    .join(format!("node_modules/{SUCCESS}/generated-by-install.js"))
                    .exists(),
            );
            assert!(!text.contains("successful-root-output"));
            assert!(!text.contains("install$"));
            assert!(!text.contains("install: Done"));
        }
    }
}

#[test]
fn quiet_ignored_builds_keep_strict_enforcement() {
    for level in ["warn", "error"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({ "dependencies": { SUCCESS: "1.0.0" } }).to_string(),
        )
        .unwrap();
        append_workspace_yaml_key(&fixture.workspace, "optimisticRepeatInstall", false);
        set_strict_dep_builds(&fixture.workspace, false);
        let output = install(&fixture.workspace, level, false);
        let text = output_text(&output);
        assert!(output.status.success());
        assert_eq!(text.contains("Ignored build scripts"), level == "warn");
        assert!(
            !fixture.workspace
                .join(format!("node_modules/{SUCCESS}/generated-by-install.js"))
                .exists(),
        );
        let yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
        let yaml = fs::read_to_string(&yaml_path)
            .unwrap()
            .replace("strictDepBuilds: false", "strictDepBuilds: true");
        fs::write(yaml_path, yaml).unwrap();
        let output = install(&fixture.workspace, level, true);
        let text = output_text(&output);
        assert!(!output.status.success());
        assert!(text.contains("Ignored build scripts"));
    }
}

#[test]
fn quiet_explicit_reporters_and_direct_run_preserve_their_contracts() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    fs::write(fixture.workspace.join("package.json"), serde_json::json!({
        "scripts": { "postinstall": "node diagnostics.cjs", "test": r#"node -e "console.log('direct-run-output')""# },
    }).to_string()).unwrap();
    fs::write(fixture.workspace.join("diagnostics.cjs"),
        "console.log('root-reporter-output'); console.error('root-reporter-error'); process.exit(1)").unwrap();
    let output = pacquet_in(&fixture.workspace)
        .args(["install", "--loglevel=warn", "--reporter=silent"])
        .output()
        .unwrap();
    let text = output_text(&output);
    assert_eq!(output.status.code(), Some(1));
    assert!(!text.contains("postinstall$"));
    let output = pacquet_in(&fixture.workspace)
        .args(["install", "--loglevel=warn", "--reporter=ndjson"])
        .output()
        .unwrap();
    output_text(&output);
    assert_eq!(output.status.code(), Some(1));
    let records = ndjson_records(&output);
    dbg!(&records);
    for line in ["root-reporter-output", "root-reporter-error"] {
        assert!(
            records
                .iter()
                .any(|record| record["name"] == "pnpm:lifecycle" && record["line"] == line),
        );
    }
    assert!(
        records
            .iter()
            .any(|record| record["name"] == "pnpm:lifecycle" && record["exitCode"] == 1),
    );
    fs::write(
        fixture.workspace.join("package.json"),
        serde_json::json!({ "scripts": { "test": r#"node -e "console.log('direct-run-output')""# } }).to_string(),
    ).unwrap();
    for level in ["warn", "error"] {
        let output = pacquet_in(&fixture.workspace)
            .args(["--loglevel", level, "run", "test"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", output_text(&output));
        assert!(output_text(&output).contains("direct-run-output"));
    }
}

fn parallel_build_script(workspace: &Path, name: &str) -> String {
    let root = serde_json::to_string(workspace.to_str().unwrap()).unwrap();
    let other = if name == "failure" { "success" } else { "failure" };
    let exit = usize::from(name == "failure");
    format!(
        r"const fs = require('fs');
const root = {root};
const name = '{name}';
const other = '{other}';
fs.writeFileSync(root + '/ready-' + name, 'yes');
const deadline = Date.now() + 20000;
function finish () {{
  if (!fs.existsSync(root + '/ready-' + other)) {{
    if (Date.now() > deadline) process.exit(2);
    setImmediate(finish); return;
  }}
  fs.writeFileSync(root + '/completed-' + name, 'yes');
  fs.writeFileSync('built', 'yes');
  console.log('parallel-' + name + '-stdout'); console.error('parallel-' + name + '-stderr');
  process.exit({exit});
}}
finish();",
    )
}

fn parallel_build_fixture(workspace: &Path) {
    fs::write(workspace.join("package.json"), serde_json::json!({
        "optionalDependencies": { "parallel-failure": "file:failure", "parallel-success": "file:success" },
    }).to_string()).unwrap();
    for name in ["failure", "success"] {
        let dir = workspace.join(name);
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": format!("parallel-{name}"), "version": "1.0.0",
                "scripts": { "postinstall": "node build.cjs" },
            })
            .to_string(),
        )
        .unwrap();
        fs::write(dir.join("build.cjs"), parallel_build_script(workspace, name)).unwrap();
    }
    allow_builds(
        workspace,
        &[("parallel-failure@file:failure", true), ("parallel-success@file:success", true)],
    );
}

#[test]
fn parallel_dependency_builds_isolate_failed_and_successful_output() {
    for level in ["warn", "error"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        parallel_build_fixture(&fixture.workspace);
        let output = pacquet_in(&fixture.workspace)
            .args(["install", "--loglevel", level, "--child-concurrency=2"])
            .output()
            .unwrap();
        let text = output_text(&output);
        assert!(output.status.success());
        for name in ["failure", "success"] {
            assert_eq!(
                fs::read_to_string(fixture.workspace.join(format!("completed-{name}"))).unwrap(),
                "yes",
            );
        }
        assert_eq!(
            fs::read_to_string(fixture.workspace.join("node_modules/parallel-success/built"))
                .unwrap(),
            "yes",
        );
        for stream in ["stdout", "stderr"] {
            assert_eq!(text.contains(&format!("parallel-failure-{stream}")), level == "warn");
            assert!(!text.contains(&format!("parallel-success-{stream}")));
        }
    }
}
