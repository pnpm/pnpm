use super::{output_of, pending_skills, pnpm};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path};

fn write_skill_package(dir: &Path, name: Option<&str>, skills: &[&str]) {
    fs::create_dir_all(dir).expect("create skill package dir");
    if let Some(name) = name {
        let manifest = serde_json::json!({ "name": name, "version": "1.0.0" });
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
    for skill in skills {
        let skill_dir = dir.join("skills").join(skill);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("SKILL.md"), format!("---\nname: {skill}\n---\n"))
            .expect("write SKILL.md");
    }
}

#[test]
fn workspace_star_links_approves_and_prunes_skills_for_subproject_and_root() {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    write_skill_package(
        &workspace.join("packages/my-skills"),
        Some("@example/my-skills"),
        &["demo-skill"],
    );
    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create packages/app");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "@example/my-skills": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write app package.json");
    fs::create_dir_all(workspace.join(".agents/skills")).expect("create .agents/skills");

    let output = output_of(pnpm(&workspace).with_arg("install"));
    assert!(output.contains("Agent skills awaiting approval: @example/my-skills."), "{output}");
    assert_eq!(
        pending_skills(&workspace),
        Some(vec!["@example/my-skills@link:packages/my-skills".to_string()]),
    );
    assert!(!workspace.join(".agents/skills/demo-skill").exists());

    pnpm(&workspace)
        .with_args(["approve", "@example/my-skills"])
        .assert()
        .success();

    assert!(workspace.join(".agents/skills/demo-skill/SKILL.md").is_file());
    assert_eq!(pending_skills(&workspace), None);
    assert_eq!(
        fs::read_to_string(workspace.join(".agents/skills/.gitignore")).expect("read .gitignore"),
        "# pnpm\n/demo-skill\n# pnpm end\n",
    );

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let revoked = fs::read_to_string(&yaml_path)
        .expect("read workspace yaml")
        .replace("skills: true", "skills: false");
    fs::write(&yaml_path, revoked).expect("write revoked workspace yaml");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join(".agents/skills/demo-skill").exists());
    assert_eq!(pending_skills(&workspace), None);
    drop(harness);
}

#[test]
fn injected_workspace_dependency_links_and_prunes_skills() {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        concat!(
            "packages:\n  - packages/*\n",
            "dedupeInjectedDeps: false\n",
            "permissions:\n  '@example/my-skills':\n    skills: true\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    write_skill_package(
        &workspace.join("packages/my-skills"),
        Some("@example/my-skills"),
        &["injected-skill"],
    );
    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create packages/app");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "@example/my-skills": "workspace:*" },
            "dependenciesMeta": { "@example/my-skills": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write app package.json");
    fs::create_dir_all(workspace.join(".agents/skills")).expect("create .agents/skills");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join(".agents/skills/injected-skill/SKILL.md").is_file());

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let revoked = fs::read_to_string(&yaml_path)
        .expect("read workspace yaml")
        .replace("skills: true", "skills: false");
    fs::write(&yaml_path, revoked).expect("write revoked workspace yaml");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join(".agents/skills/injected-skill").exists());
    drop(harness);
}

#[test]
fn link_and_aliased_file_directory_dependencies_approve_link_and_prune() {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    let parent = workspace.parent().expect("workspace parent");
    let linked_ext = parent.join("linked-skills-only");
    let file_ext = parent.join("file-skills-pkg");
    write_skill_package(&linked_ext, None, &["shared-skill"]);
    write_skill_package(&file_ext, Some("@example/file-skills"), &["shared-skill"]);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@example/linked-skills": "link:../linked-skills-only",
                "@app/custom-alias": "file:../file-skills-pkg",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join(".agents/skills")).expect("create .agents/skills");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join(".agents/skills/shared-skill").exists());

    pnpm(&workspace)
        .with_args(["approve", "--all"])
        .assert()
        .success();

    assert!(
        workspace
            .join(".agents/skills/pnpm-@example+linked-skills-shared-skill/SKILL.md")
            .is_file(),
    );
    assert!(
        workspace.join(".agents/skills/pnpm-@app+custom-alias-shared-skill/SKILL.md").is_file(),
    );

    pnpm(&workspace)
        .with_args(["remove", "@example/linked-skills"])
        .assert()
        .success();

    assert!(!workspace.join(".agents/skills/pnpm-@example+linked-skills-shared-skill").exists());
    assert!(workspace.join(".agents/skills/shared-skill/SKILL.md").is_file());
    drop(harness);
}

#[test]
fn workspace_skills_link_across_hoisted_gvs_and_per_project_lockfiles() {
    for extra_yaml in ["nodeLinker: hoisted\n", "enableGlobalVirtualStore: true\n"] {
        let harness = CommandTempCwd::init().add_mocked_registry();
        let workspace = harness.workspace.clone();
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!(
                "packages:\n  - packages/*\n{extra_yaml}permissions:\n  '@example/my-skills':\n    skills: true\n",
            ),
        )
        .expect("write pnpm-workspace.yaml");
        fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
            .expect("write root package.json");
        write_skill_package(
            &workspace.join("packages/my-skills"),
            Some("@example/my-skills"),
            &["demo-skill"],
        );
        let app_dir = workspace.join("packages/app");
        fs::create_dir_all(&app_dir).expect("create packages/app");
        fs::write(
            app_dir.join("package.json"),
            r#"{"name":"app","dependencies":{"@example/my-skills":"workspace:*"}}"#,
        )
        .expect("write app package.json");
        fs::create_dir_all(workspace.join(".agents/skills")).expect("create .agents/skills");

        pnpm(&workspace)
            .with_arg("install")
            .assert()
            .success();
        assert!(
            workspace.join(".agents/skills/demo-skill/SKILL.md").is_file(),
            "failed for config:\n{extra_yaml}",
        );
        drop(harness);
    }

    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root package.json");
    write_skill_package(
        &workspace.join("packages/my-skills"),
        Some("@example/my-skills"),
        &["demo-skill"],
    );
    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(app_dir.join(".agents/skills")).expect("create app .agents/skills");
    fs::write(
        app_dir.join("package.json"),
        r#"{"name":"app","dependencies":{"@example/my-skills":"workspace:*"}}"#,
    )
    .expect("write app package.json");

    pnpm(&app_dir)
        .with_arg("install")
        .assert()
        .success();
    assert!(!app_dir.join(".agents/skills/demo-skill").exists());

    pnpm(&app_dir)
        .with_args(["approve", "@example/my-skills"])
        .assert()
        .success();
    assert!(app_dir.join(".agents/skills/demo-skill/SKILL.md").is_file());
    drop(harness);
}

#[test]
fn local_tarball_dependency_is_not_approved_by_bare_package_name() {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    let pkg_src = workspace
        .parent()
        .expect("workspace parent")
        .join("tarball-src");
    write_skill_package(&pkg_src, Some("@example/my-skills"), &["tar-skill"]);

    pnpm(&pkg_src)
        .with_args(["pack", "--pack-destination"])
        .with_arg(&workspace)
        .assert()
        .success();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "permissions:\n  '@example/my-skills':\n    skills: true\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "dependencies": { "@example/my-skills": "file:./example-my-skills-1.0.0.tgz" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join(".agents/skills")).expect("create .agents/skills");

    pnpm(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join(".agents/skills/tar-skill").exists());
    assert_eq!(
        pending_skills(&workspace),
        Some(vec!["@example/my-skills@file:example-my-skills-1.0.0.tgz".to_string()]),
    );
    drop(harness);
}
