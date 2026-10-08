use super::{AgentSkillsError, AgentSkillsState, SyncAgentSkills, sync_agent_skills};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

const ALL_GROUPS: IncludedDependencies = IncludedDependencies {
    dependencies: true,
    dev_dependencies: true,
    optional_dependencies: true,
};

/// A workspace whose root project depends on `deps` (`alias`, `version`,
/// skills shipped).
fn workspace(deps: &[(&str, &str, &[&str])]) -> (TempDir, Lockfile) {
    let root = tempfile::tempdir().expect("create workspace");
    let mut lockfile =
        String::from("lockfileVersion: '9.0'\nimporters:\n  .:\n    devDependencies:\n");
    for (alias, version, skills) in deps {
        writeln!(
            lockfile,
            "      '{alias}':\n        specifier: {version}\n        version: {version}",
        )
        .expect("write to a String");
        install_package(
            &root
                .path()
                .join("node_modules")
                .join(alias),
            skills,
        );
    }
    (root, serde_saphyr::from_str(&lockfile).expect("parse lockfile"))
}

fn install_package(dir: &Path, skills: &[&str]) {
    fs::create_dir_all(dir).expect("create package dir");
    for skill in skills {
        let skill_dir = dir.join("skills").join(skill);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("SKILL.md"), "# skill\n").expect("write SKILL.md");
    }
}

fn config(allow_skills: &[(&str, bool)], skills_dirs: Option<&[&str]>) -> Config {
    Config {
        allow_skills: allow_skills
            .iter()
            .map(|(pkg, allowed)| (pkg.to_string(), *allowed))
            .collect::<HashMap<_, _>>(),
        skills_dirs: skills_dirs.map(|dirs| {
            dirs.iter()
                .map(ToString::to_string)
                .collect()
        }),
        ..Config::default()
    }
}

fn sync(
    root: &Path,
    lockfile: &Lockfile,
    config: &Config,
    linked: &[String],
    agent_dir: Option<&str>,
) -> Result<AgentSkillsState, AgentSkillsError> {
    sync_agent_skills(&SyncAgentSkills {
        config,
        workspace_root: root,
        lockfile,
        included: ALL_GROUPS,
        linked,
        agent_dir,
    })
}

fn link_target(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("resolve link")
}

#[test]
fn records_unapproved_skills_as_pending() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"]), ("bar", "1.0.0", &[])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state = sync(root.path(), &lockfile, &config(&[], None), &[], None).unwrap();

    assert_eq!(state, AgentSkillsState { pending: vec!["foo@1.0.0".to_string()], linked: vec![] });
    assert_eq!(fs::read_dir(root.path().join(".claude/skills")).unwrap().count(), 0);
}

#[test]
fn links_approved_skills_into_existing_agent_dirs() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide", "migrate"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    fs::create_dir_all(root.path().join(".cursor/skills")).unwrap();
    fs::write(root.path().join(".claude/skills/.gitignore"), "local-*").unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.pending, Vec::<String>::new());
    assert_eq!(
        state.linked,
        [
            ".claude/skills/pnpm-foo-guide",
            ".claude/skills/pnpm-foo-migrate",
            ".cursor/skills/pnpm-foo-guide",
            ".cursor/skills/pnpm-foo-migrate",
        ],
    );
    assert_eq!(
        link_target(&root.path().join(".claude/skills/pnpm-foo-guide")),
        link_target(&root.path().join("node_modules/foo/skills/guide")),
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".claude/skills/.gitignore")).unwrap(),
        "local-*\npnpm-*\n",
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".cursor/skills/.gitignore")).unwrap(),
        "pnpm-*\n",
    );
}

#[test]
fn creates_the_dir_of_the_agent_named_by_the_environment() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);

    let state =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], Some(".claude/skills"))
            .unwrap();

    assert_eq!(state.linked, [".claude/skills/pnpm-foo-guide"]);
    assert!(
        root.path()
            .join(".claude/skills/pnpm-foo-guide/SKILL.md")
            .is_file(),
    );
}

#[test]
fn explicit_dirs_replace_detection() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state = sync(
        root.path(),
        &lockfile,
        &config(&[("foo", true)], Some(&["agents/skills"])),
        &[],
        Some(".gemini/skills"),
    )
    .unwrap();

    assert_eq!(state.linked, ["agents/skills/pnpm-foo-guide"]);
    assert!(!root.path().join(".gemini").exists());
}

#[test]
fn fails_when_an_approved_skill_has_no_dir() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);

    let error = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None)
        .expect_err("an approved skill must be linked somewhere");

    assert!(matches!(error, AgentSkillsError::NoTargetDir { ref packages } if packages == "foo"));
}

#[test]
fn denied_skills_are_neither_pending_nor_linked() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", false)], None), &[], None).unwrap();

    assert_eq!(state, AgentSkillsState::default());
}

#[test]
fn prunes_entries_that_are_no_longer_approved() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills/hand-written")).unwrap();
    let linked =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap().linked;
    assert!(
        root.path()
            .join(".claude/skills/pnpm-foo-guide")
            .exists(),
    );

    let state = sync(root.path(), &lockfile, &config(&[], None), &linked, None).unwrap();

    assert_eq!(state.pending, ["foo@1.0.0"]);
    assert!(
        !root
            .path()
            .join(".claude/skills/pnpm-foo-guide")
            .exists(),
    );
    assert!(
        root.path()
            .join(".claude/skills/hand-written")
            .exists(),
    );
}

#[test]
fn prunes_only_recorded_links() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let skills = root.path().join(".claude/skills");
    fs::create_dir_all(skills.join("pnpm-replaced")).unwrap();
    fs::create_dir_all(skills.join("hand-written")).unwrap();
    fs::create_dir_all(root.path().join("pnpm-outside")).unwrap();
    let recorded = [
        ".claude/skills/pnpm-replaced".to_string(),
        ".claude/skills/hand-written".to_string(),
        ".claude/skills/../../pnpm-outside".to_string(),
    ];

    sync(root.path(), &lockfile, &config(&[], None), &recorded, None).unwrap();

    assert!(skills.join("pnpm-replaced").exists());
    assert!(skills.join("hand-written").exists());
    assert!(
        root.path()
            .join("pnpm-outside")
            .exists(),
    );
}

#[test]
fn an_empty_dirs_list_turns_skills_off() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    let linked =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap().linked;

    let state =
        sync(root.path(), &lockfile, &config(&[("foo", true)], Some(&[])), &linked, None).unwrap();

    assert_eq!(state, AgentSkillsState::default());
    assert!(
        !root
            .path()
            .join(".claude/skills/pnpm-foo-guide")
            .exists(),
    );
}

#[test]
fn never_replaces_an_entry_pnpm_did_not_link() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let occupied = root.path().join(".claude/skills/pnpm-foo-guide");
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("SKILL.md"), "mine").unwrap();

    let error = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None)
        .expect_err("an unrecorded entry must not be replaced");

    assert!(matches!(error, AgentSkillsError::Occupied { .. }));
    assert_eq!(fs::read_to_string(occupied.join("SKILL.md")).unwrap(), "mine");
}

#[test]
fn escapes_the_scope_of_a_package() {
    let (root, lockfile) = workspace(&[("@acme/kit", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state =
        sync(root.path(), &lockfile, &config(&[("@acme/kit", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".claude/skills/pnpm-@acme+kit-guide"]);
}

#[test]
fn the_highest_version_decides() {
    let root = tempfile::tempdir().unwrap();
    let lockfile: Lockfile = serde_saphyr::from_str(concat!(
        "lockfileVersion: '9.0'\n",
        "importers:\n",
        "  .:\n",
        "    dependencies:\n",
        "      foo:\n",
        "        specifier: 1.0.0\n",
        "        version: 1.0.0\n",
        "  packages/app:\n",
        "    dependencies:\n",
        "      foo:\n",
        "        specifier: 2.0.0\n",
        "        version: 2.0.0\n",
    ))
    .unwrap();
    install_package(&root.path().join("node_modules/foo"), &["guide"]);
    install_package(&root.path().join("packages/app/node_modules/foo"), &[]);

    let state = sync(root.path(), &lockfile, &config(&[], None), &[], None).unwrap();

    assert_eq!(state, AgentSkillsState::default());
}

#[cfg(unix)]
#[test]
fn links_once_into_dirs_that_are_one_directory() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".agents/skills")).unwrap();
    fs::create_dir_all(root.path().join(".claude")).unwrap();
    std::os::unix::fs::symlink("../.agents/skills", root.path().join(".claude/skills")).unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".agents/skills/pnpm-foo-guide"]);
}
