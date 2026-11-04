use crate::{AllowBuild, Config, NAMED_UNRECOGNIZED_SETTINGS, SkillsPermission, WorkspaceSettings};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::Path};

fn parse(yaml: &str) -> WorkspaceSettings {
    serde_saphyr::from_str(yaml).expect("parse pnpm-workspace.yaml")
}

fn applied(yaml: &str) -> Config {
    let mut config = Config::default();
    parse(yaml).apply_to(&mut config, Path::new("/workspace"));
    config
}

#[test]
fn folds_build_permissions_into_allow_builds() {
    let config = applied(
        "allowBuilds:\n  esbuild: true\n  sharp: false\npermissions:\n  sharp:\n    build: true\n  drizzle-kit:\n    build: false\n    skills: true\n",
    );
    assert_eq!(
        config.allow_builds,
        HashMap::from([
            ("esbuild".to_string(), true),
            ("sharp".to_string(), true),
            ("drizzle-kit".to_string(), false),
        ]),
    );
    assert_eq!(
        config.allow_skills,
        HashMap::from([("drizzle-kit".to_string(), SkillsPermission::All(true))]),
    );
}

#[test]
fn skill_choices_are_recorded_as_chosen() {
    let config = applied("permissions:\n  drizzle-kit:\n    skills: [guide, migrate]\n");
    assert_eq!(
        config.allow_skills,
        HashMap::from([(
            "drizzle-kit".to_string(),
            SkillsPermission::Chosen(vec!["guide".to_string(), "migrate".to_string()]),
        )]),
    );
    let denied = applied("permissions:\n  drizzle-kit:\n    skills: false\n");
    assert_eq!(
        denied.allow_skills,
        HashMap::from([("drizzle-kit".to_string(), SkillsPermission::All(false))]),
    );
}

#[test]
fn undecided_permissions_decide_nothing() {
    let config = applied("permissions:\n  esbuild:\n    build: set this to true or false\n");
    assert_eq!(config.allow_builds, HashMap::new());
}

#[test]
fn skills_permission_queries() {
    let all_true = SkillsPermission::All(true);
    assert_eq!(all_true.as_all(), Some(true));
    assert!(all_true.links_any());
    assert!(all_true.links_skill("guide"));

    let all_false = SkillsPermission::All(false);
    assert_eq!(all_false.as_all(), Some(false));
    assert!(!all_false.links_any());
    assert!(!all_false.links_skill("guide"));

    let chosen = SkillsPermission::Chosen(vec!["guide".to_string()]);
    assert_eq!(chosen.as_all(), None);
    assert!(chosen.links_any());
    assert!(chosen.links_skill("guide"));
    assert!(!chosen.links_skill("migrate"));

    let empty_chosen = SkillsPermission::Chosen(Vec::new());
    assert!(!empty_chosen.links_any());
}

#[test]
fn resolved_settings_maps_allow_skills() {
    let mut config = Config::default();
    config.allow_skills.insert(
        "drizzle-kit".to_string(),
        SkillsPermission::Chosen(vec!["guide".to_string()]),
    );
    let resolved = WorkspaceSettings::from_resolved(&config);
    let permissions = resolved.permissions.unwrap();
    assert_eq!(
        permissions.get("drizzle-kit").and_then(|entry| entry.skills.clone()),
        Some(SkillsPermission::Chosen(vec!["guide".to_string()])),
    );
}

#[test]
fn applies_skills_dirs() {
    let config = applied("skills:\n  dirs:\n    - .claude/skills\n");
    assert_eq!(config.skills_dirs, Some(vec![".claude/skills".to_string()]));
    let config = applied("skills:\n  dirs: []\n");
    assert_eq!(config.skills_dirs, Some(Vec::new()));
}

#[test]
fn names_a_bounded_number_of_unknown_capabilities() {
    let capabilities = (0..=NAMED_UNRECOGNIZED_SETTINGS)
        .map(|index| format!("    cap{index}: true\n"))
        .collect::<Vec<_>>()
        .concat();
    let yaml = format!("permissions:\n  esbuild:\n{capabilities}");
    let mut settings = parse(&yaml);
    settings.collect_key_issues(&yaml);
    let report = settings.key_issues.unrecognized_permissions;
    assert_eq!(report.named.len(), NAMED_UNRECOGNIZED_SETTINGS);
    assert_eq!(report.total, NAMED_UNRECOGNIZED_SETTINGS + 1);
}

#[test]
fn names_the_unknown_capabilities() {
    let yaml = "permissions:\n  esbuild:\n    build: true\n    mcp: true\n";
    let mut settings = parse(yaml);
    settings.collect_key_issues(yaml);
    assert_eq!(settings.key_issues.unrecognized_permissions.named, ["permissions['esbuild'].mcp"]);
    assert_eq!(settings.key_issues.unrecognized_permissions.total, 1);
    let permissions = settings.permissions.expect("permissions");
    assert_eq!(permissions["esbuild"].build, Some(AllowBuild::Decided(true)));
    assert!(permissions["esbuild"].unknown.is_empty());
}

#[test]
fn decided_build_approvals_prefers_permissions() {
    let settings =
        parse("allowBuilds:\n  esbuild: false\npermissions:\n  esbuild:\n    build: true\n");
    assert_eq!(
        settings.decided_build_approvals(),
        Some(HashMap::from([("esbuild".to_string(), true)])),
    );
    assert_eq!(settings.decided_skill_approvals(), Some(HashMap::new()));
    assert_eq!(parse("{}").decided_build_approvals(), None);
}
