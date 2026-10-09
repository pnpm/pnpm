//! Agent skills shipped by direct dependencies: recorded as awaiting
//! approval on install, linked into the agent skill directories once
//! `permissions` approves them, and pruned once it no longer does.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

const PACKAGE: &str = "@pnpm.e2e/with-agent-skills";
const LINK: &str = ".claude/skills/pnpm-@pnpm.e2e+with-agent-skills-guide";

/// `pnpm` rooted at `workspace`, with no agent identifying itself.
fn pnpm(workspace: &Path) -> Command {
    let mut command = Command::cargo_bin("pnpm").expect("find the pnpm binary");
    for var in [
        "CLAUDECODE",
        "CLAUDE_CODE",
        "CURSOR_AGENT",
        "GEMINI_CLI",
        "ANTIGRAVITY_AGENT",
        "COPILOT_AGENT",
        "COPILOT_CLI",
        "CODEX_THREAD_ID",
        "CODEX_SANDBOX",
        "AI_AGENT",
    ] {
        command.env_remove(var);
    }
    command.with_current_dir(workspace)
}

/// A project that depends on the skills fixture and has a
/// `.claude/skills` directory, with `workspace_yaml` as its settings.
fn project(workspace_yaml: &str) -> (CommandTempCwd<AddMockedRegistry>, std::path::PathBuf) {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    let package_json = serde_json::json!({ "devDependencies": { PACKAGE: "1.0.0" } });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let existing = fs::read_to_string(&yaml_path).unwrap_or_default();
    fs::write(&yaml_path, format!("{existing}{workspace_yaml}"))
        .expect("write pnpm-workspace.yaml");
    fs::create_dir_all(workspace.join(".claude/skills")).expect("create .claude/skills");
    (harness, workspace)
}

fn output_of(mut command: Command) -> String {
    let output = command
        .assert()
        .success()
        .get_output()
        .clone();
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

fn pending_skills(workspace: &Path) -> Option<Vec<String>> {
    pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(&workspace.join(
        "node_modules",
    ))
    .expect("read .modules.yaml")
    .expect(".modules.yaml exists")
    .pending_skills
    .map(|pending| {
        pending
            .into_iter()
            .map(|dep_path| dep_path.as_str().to_string())
            .collect()
    })
}

#[test]
fn install_records_unapproved_skills_and_links_nothing() {
    let (harness, workspace) = project("");

    let output = output_of(pnpm(&workspace).with_arg("install"));

    assert!(
        output.contains("Agent skills awaiting approval: @pnpm.e2e/with-agent-skills."),
        "{output}",
    );
    assert_eq!(pending_skills(&workspace), Some(vec![format!("{PACKAGE}@1.0.0")]));
    assert!(!workspace.join(LINK).exists());
    drop(harness);
}

#[test]
fn approve_links_the_skills_and_records_the_grant() {
    let (harness, workspace) = project("");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    pnpm(&workspace)
        .with_args(["approve", PACKAGE])
        .assert()
        .success();

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(
        yaml.contains("permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n"),
        "{yaml}",
    );
    assert!(
        workspace
            .join(LINK)
            .join("SKILL.md")
            .is_file(),
    );
    assert!(
        workspace
            .join(LINK)
            .join("reference.md")
            .is_file(),
    );
    let entries: Vec<String> = fs::read_dir(workspace.join(".claude/skills"))
        .expect("read .claude/skills")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries.len(), 2, "only the guide skill and the .gitignore: {entries:?}");
    assert_eq!(
        fs::read_to_string(workspace.join(".claude/skills/.gitignore")).expect("read .gitignore"),
        "pnpm-*\n",
    );
    assert_eq!(pending_skills(&workspace), None);

    let list = output_of(pnpm(&workspace).with_arg("permissions"));
    assert_eq!(list, "Granted:\n  @pnpm.e2e/with-agent-skills  skills\n");
    drop(harness);
}

#[test]
fn install_links_approved_skills_and_prunes_revoked_ones() {
    let (harness, workspace) =
        project("permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        workspace
            .join(LINK)
            .join("SKILL.md")
            .is_file(),
    );

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml =
        fs::read_to_string(&yaml_path).expect("read yaml").replace("skills: true", "skills: false");
    fs::write(&yaml_path, yaml).expect("write yaml");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(!workspace.join(LINK).exists());
    assert_eq!(pending_skills(&workspace), None);
    drop(harness);
}

#[test]
fn agents_named_by_the_environment_get_their_directories() {
    for (env_var, dir) in [
        ("CLAUDECODE", ".claude/skills"),
        ("ANTIGRAVITY_AGENT", ".agents/skills"),
        ("COPILOT_AGENT", ".github/skills"),
    ] {
        let (harness, workspace) =
            project("permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");
        fs::remove_dir_all(workspace.join(".claude")).expect("remove .claude");

        pnpm(&workspace)
            .with_env(env_var, "1")
            .with_arg("install")
            .assert()
            .success();

        assert!(
            workspace
                .join(dir)
                .join("pnpm-@pnpm.e2e+with-agent-skills-guide")
                .join("SKILL.md")
                .is_file(),
            "skill linked for {env_var} in {dir}",
        );
        drop(harness);
    }
}

#[test]
fn an_approved_skill_without_a_directory_fails_the_install() {
    let (harness, workspace) =
        project("permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");
    fs::remove_dir_all(workspace.join(".claude")).expect("remove .claude");

    let output = pnpm(&workspace)
        .with_arg("install")
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("ERR_PNPM_NO_AGENT_SKILLS_DIR")
            || stdout.contains("ERR_PNPM_NO_AGENT_SKILLS_DIR"),
        "stdout: {stdout}\nstderr: {stderr}",
    );
    drop(harness);
}

#[test]
fn approve_builds_writes_into_an_existing_permissions_block() {
    let (harness, workspace) = project(
        "strictDepBuilds: false\npermissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: false\n",
    );
    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/install-script-example": "1.0.0" },
        "devDependencies": { PACKAGE: "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    pnpm(&workspace)
        .with_args(["approve-builds", "@pnpm.e2e/install-script-example"])
        .assert()
        .success();

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(yaml.contains("  '@pnpm.e2e/install-script-example':\n    build: true\n"), "{yaml}");
    assert!(!yaml.contains("allowBuilds"), "{yaml}");
    drop(harness);
}

#[test]
fn repeat_install_links_into_a_new_agent_dir() {
    let (harness, workspace) =
        project("permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::create_dir_all(workspace.join(".cursor/skills")).expect("create .cursor/skills");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(
        workspace.join(".cursor/skills/pnpm-@pnpm.e2e+with-agent-skills-guide/SKILL.md").is_file(),
    );
    drop(harness);
}
