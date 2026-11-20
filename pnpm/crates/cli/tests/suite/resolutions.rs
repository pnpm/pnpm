use crate::auto_dedupe::pnpm_at;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

fn write_resolutions_and_overrides(workspace: &Path) {
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "resolutions-host",
            "version": "1.0.0",
            "resolutions": {
                "@pnpm.e2e/foo": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json with resolutions");
    let mut yaml = OpenOptions::new()
        .append(true)
        .open(workspace.join("pnpm-workspace.yaml"))
        .expect("open pnpm-workspace.yaml for append");
    writeln!(yaml, "overrides:").expect("append overrides key");
    writeln!(yaml, r#"  "@pnpm.e2e/bar": "2.0.0""#).expect("append override entry");
}

fn write_manifest_with_resolutions(workspace: &Path) {
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "resolutions-warning-host",
            "version": "1.0.0",
            "resolutions": {
                "@pnpm.e2e/foo": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json with resolutions");
}

fn contains_pnpm_warn_log(stderr: &str, expected: &str) -> bool {
    stderr
        .lines()
        .any(|line| {
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
                return false;
            };
            parsed.get("name").and_then(|v| v.as_str()) == Some("pnpm")
                && parsed.get("level").and_then(|v| v.as_str()) == Some("warn")
                && parsed
                    .get("message")
                    .and_then(|v| v.as_str())
                    .is_some_and(|msg| msg.contains(expected))
        })
}

#[test]
fn add_succeeds_and_warns_when_resolutions_and_overrides_both_exist() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_resolutions_and_overrides(&workspace);

    let output = pacquet
        .with_args(["add", "@pnpm.e2e/hello-world-js-bin", "--reporter=ndjson"])
        .output()
        .expect("spawn pacquet add");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`pacquet add` must succeed when both resolutions and overrides exist\nstderr:\n{stderr}",
    );

    assert!(
        contains_pnpm_warn_log(&stderr, r#""resolutions" field in package.json is ignored"#),
        "`pacquet add` must emit the 'resolutions ignored' warning when both fields exist\nstderr:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn install_resolutions_warning_routed_through_silent_reporter_produces_no_stderr() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest_with_resolutions(&workspace);

    let output = pacquet
        .with_args(["install", "--reporter=silent"])
        .output()
        .expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("resolutions"),
        "`--reporter=silent` must not leak the resolutions deprecation warning to stderr\nstderr:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn install_resolutions_warning_routed_through_ndjson_reporter_emits_parseable_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest_with_resolutions(&workspace);

    let output = pacquet
        .with_args(["install", "--reporter=ndjson"])
        .output()
        .expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        contains_pnpm_warn_log(&stderr, "resolutions"),
        "ndjson reporter must emit the resolutions deprecation warning as a `pnpm` channel warn record\nstderr:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn install_succeeds_and_warns_when_resolutions_and_overrides_both_exist() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_resolutions_and_overrides(&workspace);

    let output = pacquet
        .with_args(["install", "--reporter=ndjson"])
        .output()
        .expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`pacquet install` must succeed when both resolutions and overrides exist\nstderr:\n{stderr}",
    );

    assert!(
        contains_pnpm_warn_log(&stderr, r#""resolutions" field in package.json is ignored"#),
        "ndjson reporter must emit the 'resolutions ignored' warning when both fields exist\nstderr:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn repeat_install_promotes_resolutions_once() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "resolutions-repeat-host",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0",
            },
            "resolutions": {
                "**/@pnpm.e2e/foo": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json with resolutions");

    for attempt in ["first", "repeat"] {
        let output = pnpm_at(&workspace)
            .with_arg("install")
            .output()
            .expect("spawn pnpm install");
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(output.status.success(), "{attempt} install failed\n{printed}");
        assert_eq!(
            printed.matches("We attempted to migrate your resolutions").count(),
            1,
            "{attempt} install\n{printed}",
        );
        assert!(
            !printed.contains(r#""resolutions" field in package.json is ignored"#),
            "{attempt} install must not treat the promoted resolutions as configured overrides\n{printed}",
        );
        if attempt == "repeat" {
            assert!(printed.contains("Already up to date"), "{printed}");
        }
        let lockfile =
            fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
        assert!(
            lockfile.contains("overrides:\n  '@pnpm.e2e/foo': 1.0.0"),
            "{attempt} install must record the promoted overrides\nlockfile:\n{lockfile}",
        );
    }

    drop((root, mock_instance));
}

#[test]
fn run_gate_accepts_promoted_resolutions() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "resolutions-run-host",
            "version": "1.0.0",
            "scripts": { "hello": "echo hello" },
            "resolutions": { "@pnpm.e2e/foo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json with resolutions");
    let mut yaml = OpenOptions::new()
        .append(true)
        .open(workspace.join("pnpm-workspace.yaml"))
        .expect("open pnpm-workspace.yaml for append");
    writeln!(yaml, "verifyDepsBeforeRun: error").expect("append verifyDepsBeforeRun");

    let install = pnpm_at(&workspace)
        .with_arg("install")
        .output()
        .expect("spawn pnpm install");
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));

    let run = pnpm_at(&workspace)
        .with_args(["run", "hello"])
        .output()
        .expect("spawn pnpm run");
    let printed =
        format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "{printed}");
    assert!(!printed.contains("resolutions"), "the gate must not repeat the warning\n{printed}");

    drop((root, mock_instance));
}

#[test]
fn recursive_run_gate_accepts_promoted_resolutions_with_dedicated_lockfiles() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "resolutions-recursive-host",
            "version": "1.0.0",
            "resolutions": { "@pnpm.e2e/foo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json with resolutions");
    let project = workspace.join("packages/project");
    fs::create_dir_all(&project).expect("create workspace project");
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "scripts": { "hello": "echo hello" },
        })
        .to_string(),
    )
    .expect("write project package.json");
    let mut yaml = OpenOptions::new()
        .append(true)
        .open(workspace.join("pnpm-workspace.yaml"))
        .expect("open pnpm-workspace.yaml for append");
    writeln!(
        yaml,
        "verifyDepsBeforeRun: error\nsharedWorkspaceLockfile: false\npackages:\n  - packages/*",
    )
    .expect("append workspace settings");

    let install = pnpm_at(&workspace)
        .with_args(["install", "-r"])
        .output()
        .expect("spawn pnpm install");
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));

    let run = pnpm_at(&workspace)
        .with_args(["-r", "run", "hello"])
        .output()
        .expect("spawn pnpm run");
    let printed =
        format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "{printed}");

    drop((root, mock_instance));
}

#[test]
fn run_gate_accepts_promoted_resolutions_extended_by_update_config() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "resolutions-hook-host",
            "version": "1.0.0",
            "scripts": { "hello": "echo hello" },
            "resolutions": { "@pnpm.e2e/foo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json with resolutions");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.overrides = { ...config.overrides, '@pnpm.e2e/bar': '2.0.0' }; return config } } }",
    )
    .expect("write pnpmfile");
    let mut yaml = OpenOptions::new()
        .append(true)
        .open(workspace.join("pnpm-workspace.yaml"))
        .expect("open pnpm-workspace.yaml for append");
    writeln!(yaml, "verifyDepsBeforeRun: error").expect("append verifyDepsBeforeRun");

    let install = pnpm_at(&workspace)
        .with_arg("install")
        .output()
        .expect("spawn pnpm install");
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("'@pnpm.e2e/foo': 1.0.0"), "{lockfile}");
    assert!(lockfile.contains("'@pnpm.e2e/bar': 2.0.0"), "{lockfile}");

    let run = pnpm_at(&workspace)
        .with_args(["run", "hello"])
        .output()
        .expect("spawn pnpm run");
    let printed =
        format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "{printed}");

    drop((root, mock_instance));
}
