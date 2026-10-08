use crate::{AllowBuild, Config, NAMED_UNRECOGNIZED_SETTINGS, WorkspaceSettings};
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
    assert_eq!(config.allow_skills, HashMap::from([("drizzle-kit".to_string(), true)]));
}

#[test]
fn undecided_permissions_decide_nothing() {
    let config = applied("permissions:\n  esbuild:\n    build: set this to true or false\n");
    assert_eq!(config.allow_builds, HashMap::new());
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
    let capabilities: String = (0..=NAMED_UNRECOGNIZED_SETTINGS)
        .map(|index| format!("    cap{index}: true\n"))
        .collect();
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
