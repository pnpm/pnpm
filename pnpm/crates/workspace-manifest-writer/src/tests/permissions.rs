use super::{TempDir, WORKSPACE_MANIFEST_FILENAME, fs};
use crate::PermissionValue;
use pnpm_config::{SkillsPermission, SkillsPermission::All};

fn run(original: Option<&str>, entries: &[(&str, PermissionValue)]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_permissions(
        dir.path(),
        entries
            .iter()
            .map(|(name, value)| (*name, value.clone())),
    )
    .expect("update succeeds");
    fs::read_to_string(&path).ok()
}

fn build(allowed: bool) -> PermissionValue {
    PermissionValue::Build(allowed)
}

fn all(allowed: bool) -> PermissionValue {
    PermissionValue::Skills(All(allowed))
}

fn chosen(skills: &[&str]) -> PermissionValue {
    PermissionValue::Skills(SkillsPermission::Chosen(
        skills
            .iter()
            .map(ToString::to_string)
            .collect(),
    ))
}

#[test]
fn build_decisions_go_to_allow_builds_without_a_permissions_block() {
    let out =
        run(Some("packages:\n  - pkgs/*\n"), &[("esbuild", build(true)), ("sharp", build(false))]);
    assert_eq!(
        out.as_deref(),
        Some("packages:\n  - pkgs/*\nallowBuilds:\n  esbuild: true\n  sharp: false\n"),
    );
}

#[test]
fn skills_decisions_create_the_permissions_block() {
    let out = run(None, &[("drizzle-kit", build(true)), ("drizzle-kit", all(true))]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds:\n  drizzle-kit: true\npermissions:\n  drizzle-kit:\n    skills: true\n"),
    );
}

#[test]
fn build_decisions_join_an_existing_permissions_block() {
    let out = run(
        Some(
            "allowBuilds:\n  esbuild: false\n  sharp: true\n\npermissions:\n  drizzle-kit:\n    skills: true\n",
        ),
        &[("esbuild", build(true)), ("drizzle-kit", build(false)), ("@acme/kit", all(false))],
    );
    assert_eq!(
        out.as_deref(),
        Some(concat!(
            "allowBuilds:\n  sharp: true\n\n",
            "permissions:\n",
            "  '@acme/kit':\n    skills: false\n",
            "  drizzle-kit:\n    build: false\n    skills: true\n",
            "  esbuild:\n    build: true\n",
        )),
    );
}

#[test]
fn a_chosen_list_is_written_as_a_flow_list() {
    let out = run(None, &[("drizzle-kit", chosen(&["guide", "migrate"]))]);
    assert_eq!(
        out.as_deref(),
        Some("permissions:\n  drizzle-kit:\n    skills: [guide, migrate]\n"),
    );
}

#[test]
fn removes_allow_builds_when_its_last_entry_moves() {
    let out =
        run(Some("allowBuilds:\n  esbuild: false\npermissions: {}\n"), &[("esbuild", build(true))]);
    assert_eq!(out.as_deref(), Some("permissions: { esbuild: { build: true } }\n"));
}

#[test]
fn rewrites_a_changed_decision_in_place() {
    let original = "permissions:\n  foo:\n    skills: false # reviewed\n";
    assert_eq!(
        run(Some(original), &[("foo", all(true))]).as_deref(),
        Some("permissions:\n  foo:\n    skills: true # reviewed\n"),
    );
    assert_eq!(run(Some(original), &[("foo", all(false))]).as_deref(), Some(original));
}

#[test]
fn a_chosen_list_overwrites_a_hand_written_block() {
    let original = "permissions:\n  drizzle-kit:\n    skills:\n      - guide\n";
    assert_eq!(
        run(Some(original), &[("drizzle-kit", chosen(&["migrate"]))]).as_deref(),
        Some("permissions:\n  drizzle-kit:\n    skills: [migrate]\n"),
    );
}

#[test]
fn a_chosen_list_matching_the_recorded_one_is_a_no_op() {
    let original = "permissions:\n  drizzle-kit:\n    skills: [guide, migrate]\n";
    assert_eq!(
        run(Some(original), &[("drizzle-kit", chosen(&["guide", "migrate"]))]).as_deref(),
        Some(original)
    );
}

#[test]
fn rejects_a_control_character_in_a_chosen_skill_name() {
    let dir = TempDir::new().expect("temp dir");
    let err =
        crate::set_permissions(dir.path(), [("drizzle-kit", chosen(&["guide\ninjected: true"]))])
            .expect_err("refuse the write");
    assert!(err.to_string().contains("control character"), "{err:#}");
}

#[test]
fn skills_dirs_are_upserted_into_the_block() {
    let out = run_skills_dirs(
        Some("allowBuilds:\n  esbuild: true\n"),
        &[".claude/skills".to_string(), ".agents/skills".to_string()],
    );
    assert_eq!(
        out.as_deref(),
        Some(
            "allowBuilds:\n  esbuild: true\nskills:\n  dirs:\n    - .claude/skills\n    - .agents/skills\n"
        ),
    );
}

#[test]
fn skills_dirs_replace_an_existing_entry() {
    let out = run_skills_dirs(
        Some("skills:\n  dirs:\n    - .cursor/skills\n"),
        &[".claude/skills".to_string()],
    );
    assert_eq!(out.as_deref(), Some("skills:\n  dirs:\n    - .claude/skills\n"),);
}

#[test]
fn identical_skills_dirs_are_a_no_op() {
    let original = "skills:\n  dirs:\n    - .claude/skills\n";
    assert_eq!(
        run_skills_dirs(Some(original), &[".claude/skills".to_string()]).as_deref(),
        Some(original),
    );
}

#[test]
fn no_dirs_drop_the_dirs_entry_and_the_block() {
    let out = run_skills_dirs(Some("skills:\n  dirs:\n    - .claude/skills\n"), &[]);
    assert_eq!(
        out.as_deref(),
        None,
        "an emptied manifest is removed, like every emptied root manifest"
    );
    assert_eq!(run_skills_dirs(None, &[]).as_deref(), None, "nothing to record, nothing written");
}

#[test]
fn an_absent_dirs_entry_is_not_removed() {
    let original = "skills:\n  other: value\n";
    assert_eq!(run_skills_dirs(Some(original), &[]).as_deref(), Some(original));
}

#[test]
fn rejects_a_control_character_in_a_skills_dir() {
    let dir = TempDir::new().expect("temp dir");
    let err = crate::set_skills_dirs(dir.path(), &[".claude/skills\ninjected: true".to_string()])
        .expect_err("refuse the write");
    assert!(err.to_string().contains("control character"), "{err:#}");
}

#[test]
fn a_block_valued_hand_written_dirs_is_replaced() {
    let original = "skills:\n  dirs:\n    - .cursor/skills\n    - extra/one\n";
    assert_eq!(
        run_skills_dirs(Some(original), &[".claude/skills".to_string()]).as_deref(),
        Some("skills:\n  dirs:\n    - .claude/skills\n"),
    );
}

/// Run `set_skills_dirs` against `original` (when `Some`) and return the
/// resulting file contents.
fn run_skills_dirs(original: Option<&str>, dirs: &[String]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_skills_dirs(dir.path(), dirs).expect("update succeeds");
    fs::read_to_string(&path).ok()
}
