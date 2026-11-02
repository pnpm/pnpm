//! Agent skills shipped by direct dependencies: recorded as awaiting
//! approval on install, linked into the agent skill directories once
//! `permissions` approves them, and pruned once it no longer does.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

const PACKAGE: &str = "@pnpm.e2e/with-agent-skills";
const LINK: &str = ".claude/skills/guide";
const NESTED_PACKAGE: &str = "@pnpm.e2e/with-nested-agent-skills";
const NESTED_LINK: &str = ".claude/skills/migrate";
const TWO_SKILLS_PACKAGE: &str = "@pnpm.e2e/with-two-skills";
const TWO_SKILLS_GUIDE: &str = ".claude/skills/guide";
const TWO_SKILLS_MIGRATE: &str = ".claude/skills/migrate";

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

/// A project that depends on `package`, a package shipping agent skills on
/// the mocked registry, and has a `.claude/skills` directory, with
/// `workspace_yaml` as its settings.
fn project(
    package: &str,
    workspace_yaml: &str,
) -> (CommandTempCwd<AddMockedRegistry>, std::path::PathBuf) {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    let package_json = serde_json::json!({ "devDependencies": { package: "1.0.0" } });
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
    let (harness, workspace) = project(PACKAGE, "");

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
    let (harness, workspace) = project(PACKAGE, "");
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
        "# pnpm\n/guide\n# pnpm end\n",
    );
    assert_eq!(pending_skills(&workspace), None);

    let list = output_of(pnpm(&workspace).with_arg("permissions"));
    assert_eq!(list, "Granted:\n  @pnpm.e2e/with-agent-skills  skills\n");
    drop(harness);
}

#[test]
fn install_links_approved_skills_and_prunes_revoked_ones() {
    let (harness, workspace) =
        project(PACKAGE, "permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");

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

    assert_eq!(
        pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(&workspace.join(
            "node_modules"
        ),)
        .unwrap()
        .unwrap()
        .linked_skills,
        Some(vec![LINK.to_string()]),
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
fn install_links_skills_nested_in_category_directories() {
    let (harness, workspace) = project(
        NESTED_PACKAGE,
        "permissions:\n  '@pnpm.e2e/with-nested-agent-skills':\n    skills: true\n",
    );

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        workspace
            .join(NESTED_LINK)
            .join("SKILL.md")
            .is_file(),
        "{NESTED_LINK} links the skill in the engineering category directory",
    );

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml =
        fs::read_to_string(&yaml_path).expect("read yaml").replace("skills: true", "skills: false");
    fs::write(&yaml_path, yaml).expect("write yaml");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(!workspace.join(NESTED_LINK).exists());
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
            project(PACKAGE, "permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");
        fs::remove_dir_all(workspace.join(".claude")).expect("remove .claude");

        pnpm(&workspace)
            .with_env(env_var, "1")
            .with_arg("install")
            .assert()
            .success();

        assert!(
            workspace
                .join(dir)
                .join("guide")
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
        project(PACKAGE, "permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");
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
fn install_links_only_the_skills_listed_in_the_permission() {
    let (harness, workspace) = project(
        TWO_SKILLS_PACKAGE,
        "permissions:\n  '@pnpm.e2e/with-two-skills':\n    skills: [guide]\n",
    );

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(
        workspace
            .join(TWO_SKILLS_GUIDE)
            .join("SKILL.md")
            .is_file(),
        "the listed skill links",
    );
    assert!(!workspace.join(TWO_SKILLS_MIGRATE).exists(), "the unlisted skill stays unlinked");
    drop(harness);
}

#[test]
fn approve_links_into_the_recorded_directories() {
    let (harness, workspace) =
        project(TWO_SKILLS_PACKAGE, "skills:\n  dirs:\n    - .claude/skills\n");
    fs::create_dir_all(workspace.join(".cursor/skills")).expect("create .cursor/skills");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(pending_skills(&workspace), Some(vec![format!("{TWO_SKILLS_PACKAGE}@1.0.0")]));

    pnpm(&workspace)
        .with_args(["approve", TWO_SKILLS_PACKAGE])
        .assert()
        .success();

    assert!(
        workspace
            .join(TWO_SKILLS_GUIDE)
            .join("SKILL.md")
            .is_file(),
    );
    assert!(
        workspace
            .join(TWO_SKILLS_MIGRATE)
            .join("SKILL.md")
            .is_file(),
    );
    let modules = pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(
        &workspace.join("node_modules"),
    )
    .unwrap()
    .unwrap();
    let linked = modules.linked_skills.expect("linked skills recorded");
    assert!(
        linked
            .iter()
            .all(|link| link.starts_with(".claude/skills/")),
        "links follow the recorded directories: {linked:?}",
    );
    let cursor_entries: Vec<String> = fs::read_dir(workspace.join(".cursor/skills"))
        .expect("read .cursor/skills")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(cursor_entries.is_empty(), "the unrecorded directory stays empty: {cursor_entries:?}");
    drop(harness);
}

#[test]
fn approve_builds_writes_into_an_existing_permissions_block() {
    let (harness, workspace) = project(
        PACKAGE,
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
        project(PACKAGE, "permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n");
    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::create_dir_all(workspace.join(".cursor/skills")).expect("create .cursor/skills");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(workspace.join(".cursor/skills/guide/SKILL.md").is_file());
    drop(harness);
}

#[test]
fn peer_variants_keep_one_link_target_and_replace_unrecorded_links() {
    let (harness, workspace) = project(
        PACKAGE,
        concat!(
            "packages:\n  - packages/*\n",
            "dedupePeerDependents: false\n",
            "permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\n",
            "packageExtensions:\n  '@pnpm.e2e/with-agent-skills@*':\n",
            "    peerDependencies:\n      '@pnpm.e2e/peer-a': '*'\n",
        ),
    );
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).unwrap();
    let modules_dir = workspace.join("node_modules");
    let install = |root_peer: &str, app_peer: &str, link: &str| {
        for (dir, name, peer) in [(&workspace, "root", root_peer), (&app, "app", app_peer)] {
            let manifest = serde_json::json!({
                "name": name,
                "dependencies": { PACKAGE: "1.0.0", "@pnpm.e2e/peer-a": peer },
            });
            fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
        }
        pnpm(&workspace)
            .with_args(["install", "--no-frozen-lockfile"])
            .assert()
            .success();
        fs::canonicalize(workspace.join(link)).unwrap()
    };

    let first = install("1.0.0", "1.0.1", LINK);
    let swapped = install("1.0.1", "1.0.0", LINK);
    assert_eq!(first, swapped, "swapping which importer has which variant keeps the target");

    let mut modules =
        pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(&modules_dir)
            .unwrap()
            .unwrap();
    assert_eq!(modules.linked_skills, Some(vec![LINK.to_string()]));
    modules.linked_skills = None;
    pnpm_modules_yaml::write_modules_manifest::<pnpm_modules_yaml::Host>(&modules_dir, modules)
        .unwrap();

    let after_peer_change = install("1.0.1", "1.0.1", LINK);
    assert_ne!(after_peer_change, first);
    assert!(first.is_dir(), "the variant that left the lockfile is still installed");
    drop(harness);
}
