use super::{AgentSkillsError, AgentSkillsState, SyncAgentSkills, sync_agent_skills};
use pnpm_config::{Config, SkillsPermission};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, HashMap},
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
    config_decided(
        &allow_skills
            .iter()
            .map(|(pkg, allowed)| (pkg.to_string(), SkillsPermission::All(*allowed)))
            .collect::<Vec<_>>(),
        skills_dirs,
    )
}

fn config_decided(
    allow_skills: &[(String, SkillsPermission)],
    skills_dirs: Option<&[&str]>,
) -> Config {
    Config {
        allow_skills: allow_skills
            .iter()
            .cloned()
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
        hoisted_locations: None,
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
            ".claude/skills/guide",
            ".claude/skills/migrate",
            ".cursor/skills/guide",
            ".cursor/skills/migrate",
        ],
    );
    assert_eq!(
        link_target(&root.path().join(".claude/skills/guide")),
        link_target(&root.path().join("node_modules/foo/skills/guide")),
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".claude/skills/.gitignore")).unwrap(),
        "local-*\n# pnpm\n/guide\n/migrate\n# pnpm end\n",
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".cursor/skills/.gitignore")).unwrap(),
        "# pnpm\n/guide\n/migrate\n# pnpm end\n",
    );
}

#[test]
fn chosen_skills_link_only_the_chosen_ones() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide", "migrate"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state = sync(
        root.path(),
        &lockfile,
        &config_decided(
            &[("foo".to_string(), SkillsPermission::Chosen(vec!["guide".to_string()]))],
            None,
        ),
        &[],
        None,
    )
    .unwrap();

    assert_eq!(state.pending, Vec::<String>::new());
    assert_eq!(state.linked, [".claude/skills/guide"]);
    assert!(
        !root
            .path()
            .join(".claude/skills/migrate")
            .exists(),
    );
}

#[test]
fn links_skills_nested_in_category_directories() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide", "engineering/implement"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".claude/skills/guide", ".claude/skills/implement"]);
    assert_eq!(
        link_target(&root.path().join(".claude/skills/implement")),
        link_target(&root.path().join("node_modules/foo/skills/engineering/implement")),
    );
}

#[test]
fn a_skill_name_one_package_ships_twice_links_its_first_directory() {
    let (root, lockfile) =
        workspace(&[("foo", "1.0.0", &["engineering/implement", "productivity/implement"])]);
    let skills = root.path().join(".claude/skills");
    fs::create_dir_all(&skills).unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".claude/skills/implement"]);
    assert!(
        fs::canonicalize(skills.join("implement"))
            .unwrap()
            .ends_with("skills/engineering/implement"),
    );
}

#[test]
fn creates_the_dir_of_the_agent_named_by_the_environment() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);

    let state =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], Some(".claude/skills"))
            .unwrap();

    assert_eq!(state.linked, [".claude/skills/guide"]);
    assert!(
        root.path()
            .join(".claude/skills/guide/SKILL.md")
            .is_file(),
    );
}

#[cfg(unix)]
#[test]
fn links_through_a_symlinked_workspace_path() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    let aliases = tempfile::tempdir().expect("create alias dir");
    let alias = aliases.path().join("workspace");
    std::os::unix::fs::symlink(root.path(), &alias).unwrap();

    sync(&alias, &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert!(alias.join(".claude/skills/guide/SKILL.md").is_file());
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

    assert_eq!(state.linked, ["agents/skills/guide"]);
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
            .join(".claude/skills/guide")
            .exists(),
    );

    let state = sync(root.path(), &lockfile, &config(&[], None), &linked, None).unwrap();

    assert_eq!(state.pending, ["foo@1.0.0"]);
    assert!(
        !root
            .path()
            .join(".claude/skills/guide")
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
    fs::create_dir_all(skills.join("taken")).unwrap();
    pnpm_fs::force_symlink_dir(
        &root.path().join("node_modules/foo/skills/guide"),
        &skills.join("stale"),
    )
    .unwrap();
    fs::create_dir_all(root.path().join("pnpm-outside")).unwrap();
    let recorded = [
        ".claude/skills/pnpm-replaced".to_string(),
        ".claude/skills/hand-written".to_string(),
        ".claude/skills/taken".to_string(),
        ".claude/skills/stale".to_string(),
        ".claude/skills/../../pnpm-outside".to_string(),
    ];

    sync(root.path(), &lockfile, &config(&[], None), &recorded, None).unwrap();

    assert!(skills.join("pnpm-replaced").exists());
    assert!(skills.join("hand-written").exists());
    assert!(skills.join("taken").exists());
    assert!(fs::symlink_metadata(skills.join("stale")).is_err());
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
            .join(".claude/skills/guide")
            .exists(),
    );
}

#[test]
fn a_taken_bare_name_falls_back_to_the_prefixed_one() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let occupied = root.path().join(".claude/skills/guide");
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("SKILL.md"), "mine").unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".claude/skills/pnpm-foo-guide"]);
    assert_eq!(fs::read_to_string(occupied.join("SKILL.md")).unwrap(), "mine");
    assert_eq!(
        fs::read_to_string(root.path().join(".claude/skills/.gitignore")).unwrap(),
        "# pnpm\n/pnpm-foo-guide\n# pnpm end\n",
    );
}

#[test]
fn a_prefixed_fallback_name_may_still_be_occupied() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let skills = root.path().join(".claude/skills");
    fs::create_dir_all(skills.join("guide")).unwrap();
    fs::create_dir_all(skills.join("pnpm-foo-guide")).unwrap();
    fs::write(skills.join("pnpm-foo-guide/SKILL.md"), "mine").unwrap();

    let error = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None)
        .expect_err("no entry name is left");

    assert!(matches!(error, AgentSkillsError::Occupied { .. }));
    assert_eq!(fs::read_to_string(skills.join("pnpm-foo-guide/SKILL.md")).unwrap(), "mine");
}

#[test]
fn an_occupied_fallback_keeps_the_existing_links() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"]), ("bar", "1.0.0", &["guide"])]);
    let skills = root.path().join(".claude/skills");
    fs::create_dir_all(&skills).unwrap();
    let linked =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap().linked;
    assert_eq!(linked, [".claude/skills/guide"]);
    fs::create_dir_all(skills.join("pnpm-bar-guide")).unwrap();

    let error =
        sync(root.path(), &lockfile, &config(&[("foo", true), ("bar", true)], None), &linked, None)
            .expect_err("the fallback name of bar is taken");

    assert!(matches!(error, AgentSkillsError::Occupied { .. }));
    assert!(skills.join("guide/SKILL.md").is_file());
}

#[cfg(unix)]
#[test]
fn replaces_a_dangling_link() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let skills = root.path().join(".claude/skills");
    fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(root.path().join("removed"), skills.join("guide")).unwrap();

    sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert!(skills.join("guide/SKILL.md").is_file());
}

#[test]
fn disabled_skills_are_neither_pending_nor_linked() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    let config = Config { agent_skills_disabled: true, ..config(&[], None) };

    let state = sync(root.path(), &lockfile, &config, &[], None).unwrap();

    assert_eq!(state, AgentSkillsState::default());
}

#[cfg(unix)]
#[test]
fn ignores_a_skill_that_resolves_outside_its_package() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("SKILL.md"), "# outside\n").unwrap();
    std::os::unix::fs::symlink(&outside, root.path().join("node_modules/foo/skills/escape"))
        .unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".claude/skills/guide"]);
}

/// A hoisted workspace whose `packages/b` depends on `foo@<version>`, with
/// `foo@1.0.0` shipping a skill in the root `node_modules`.
fn hoisted_workspace(version: &str) -> (TempDir, Lockfile) {
    let root = tempfile::tempdir().expect("create workspace");
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    fs::create_dir_all(root.path().join("packages/b")).unwrap();
    install_package(&root.path().join("node_modules/foo"), &["guide"]);
    let lockfile = serde_saphyr::from_str(&format!(
        "lockfileVersion: '9.0'\nimporters:\n  .: {{}}\n  packages/b:\n    dependencies:\n      foo:\n        specifier: {version}\n        version: {version}\n",
    ))
    .expect("parse lockfile");
    (root, lockfile)
}

fn sync_hoisted(
    root: &Path,
    lockfile: &Lockfile,
    allow_skills: &[(&str, bool)],
) -> AgentSkillsState {
    let locations =
        BTreeMap::from([("foo@1.0.0".to_string(), vec!["node_modules/foo".to_string()])]);
    let config =
        Config { node_linker: pnpm_config::NodeLinker::Hoisted, ..config(allow_skills, None) };
    sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root,
        lockfile,
        included: ALL_GROUPS,
        linked: &[],
        agent_dir: None,
        hoisted_locations: Some(&locations),
    })
    .unwrap()
}

#[test]
fn finds_a_dependency_hoisted_to_the_workspace_root() {
    let (root, lockfile) = hoisted_workspace("1.0.0");

    let state = sync_hoisted(root.path(), &lockfile, &[]);

    assert_eq!(state.pending, ["foo@1.0.0"]);
}

#[test]
fn a_custom_modules_dir_names_only_the_root_one() {
    let root = tempfile::tempdir().expect("create workspace");
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    install_package(&root.path().join("packages/b/node_modules/foo"), &["guide"]);
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/b:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0\n",
    )
    .expect("parse lockfile");
    let locations = BTreeMap::from([(
        "foo@1.0.0".to_string(),
        vec!["packages/b/node_modules/foo".to_string()],
    )]);
    let config = Config {
        node_linker: pnpm_config::NodeLinker::Hoisted,
        modules_dir: root.path().join("my_modules"),
        ..config(&[], None)
    };

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &lockfile,
        included: ALL_GROUPS,
        linked: &[],
        agent_dir: None,
        hoisted_locations: Some(&locations),
    })
    .unwrap();

    assert_eq!(state.pending, ["foo@1.0.0"]);
}

#[test]
fn a_hoisted_copy_of_another_version_is_not_the_dependency() {
    let (root, lockfile) = hoisted_workspace("2.0.0");

    let state = sync_hoisted(root.path(), &lockfile, &[("foo@2.0.0", true)]);

    assert_eq!(state, AgentSkillsState::default());
}

#[test]
fn an_alias_that_leaves_node_modules_is_ignored() {
    let root = tempfile::tempdir().expect("create workspace");
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    fs::create_dir_all(root.path().join("node_modules")).unwrap();
    install_package(&root.path().join("outside"), &["guide"]);
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      '../outside':\n        specifier: 1.0.0\n        version: 1.0.0\n",
    )
    .expect("parse lockfile");

    let state = sync(root.path(), &lockfile, &config(&[], None), &[], None).unwrap();

    assert_eq!(state, AgentSkillsState::default());
}

#[test]
fn a_missing_higher_version_does_not_hide_an_installed_one() {
    let root = tempfile::tempdir().expect("create workspace");
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    install_package(&root.path().join("packages/b/node_modules/foo"), &["guide"]);
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  packages/a:\n    dependencies:\n      foo:\n        specifier: 2.0.0\n        version: 2.0.0\n  packages/b:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0\n",
    )
    .expect("parse lockfile");

    let state = sync(root.path(), &lockfile, &config(&[], None), &[], None).unwrap();

    assert_eq!(state.pending, ["foo@1.0.0"]);
}

#[test]
fn a_package_missing_from_one_importer_is_found_in_another() {
    let root = tempfile::tempdir().expect("create workspace");
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    install_package(&root.path().join("packages/b/node_modules/foo"), &["guide"]);
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  packages/a:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0\n  packages/b:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0\n",
    )
    .expect("parse lockfile");

    let state = sync(root.path(), &lockfile, &config(&[], None), &[], None).unwrap();

    assert_eq!(state.pending, ["foo@1.0.0"]);
}

#[test]
fn escapes_the_scope_of_a_package() {
    let (root, lockfile) = workspace(&[("@acme/kit", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills/guide")).unwrap();

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

    assert_eq!(state.linked, [".agents/skills/guide"]);
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_gitignore() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let target_dir = root.path().join(".claude/skills");
    fs::create_dir_all(&target_dir).unwrap();
    let external = tempfile::tempdir().expect("create external tempdir");
    let external_file = external.path().join("fake-gitignore");
    fs::write(&external_file, "").unwrap();
    std::os::unix::fs::symlink(&external_file, target_dir.join(".gitignore")).unwrap();

    let error =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap_err();
    assert!(matches!(error, AgentSkillsError::Io { .. }));
    assert_eq!(fs::read_to_string(&external_file).unwrap(), "");
}

#[test]
fn peer_variants_choose_the_same_target_when_importers_change_order() {
    let root = tempfile::tempdir().unwrap();
    let variants = ["1.0.0(peer@1.0.0)", "1.0.0(peer@2.0.0)"];
    for variant in variants {
        install_package(
            &root
                .path()
                .join("node_modules/.pnpm")
                .join(variant)
                .join("foo"),
            &["guide"],
        );
    }
    let config = config(&[("foo", true)], Some(&[".claude/skills"]));
    let mut targets = Vec::new();
    let mut linked = Vec::new();
    for order in [variants, [variants[1], variants[0]]] {
        let mut yaml = String::from("lockfileVersion: '9.0'\nimporters:\n");
        for (importer, variant) in ["a", "b"].into_iter().zip(order) {
            let modules = root
                .path()
                .join(importer)
                .join("node_modules");
            fs::create_dir_all(&modules).unwrap();
            pnpm_fs::force_symlink_dir(
                &root
                    .path()
                    .join("node_modules/.pnpm")
                    .join(variant)
                    .join("foo"),
                &modules.join("foo"),
            )
            .unwrap();
            writeln!(yaml, "  {importer}:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: {variant}").unwrap();
        }
        let lockfile: Lockfile = serde_saphyr::from_str(&yaml).unwrap();
        let state = sync(root.path(), &lockfile, &config, &linked, None).unwrap();
        linked = state.linked;
        targets.push(link_target(&root.path().join(".claude/skills/guide")));
    }
    assert_eq!(targets[0], targets[1]);
}

/// A root project whose `foo` is `foo@1.0.0(peer@2.0.0)` in the virtual
/// store `node_modules/.pnpm`, with a config that names that store, and the
/// entry pnpm links for its skill named by `skill` — the bare skill name.
fn workspace_with_peer_variant(skill: &str) -> (TempDir, Lockfile, Config, String) {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("node_modules/.pnpm");
    let package = store.join("foo@1.0.0_peer@2.0.0/node_modules/foo");
    install_package(&package, &[skill]);
    pnpm_fs::force_symlink_dir(&package, &root.path().join("node_modules/foo")).unwrap();
    let yaml = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0(peer@2.0.0)\n";
    let mut config = config(&[("foo", true)], Some(&[".claude/skills"]));
    config.install_state_dir = store;
    let entry = Path::new(skill)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    (root, serde_saphyr::from_str(yaml).unwrap(), config, entry)
}

#[test]
fn replaces_an_unrecorded_link_to_another_copy_in_the_virtual_store() {
    for old_slot in ["foo@1.0.0_peer@1.0.0", "foo@0.9.0"] {
        let (root, lockfile, config, entry) = workspace_with_peer_variant("guide");
        let old_skill = root
            .path()
            .join("node_modules/.pnpm")
            .join(old_slot)
            .join("node_modules/foo");
        install_package(&old_skill, &["guide"]);
        let link = root
            .path()
            .join(".claude/skills")
            .join(&entry);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(&old_skill.join("skills/guide"), &link).unwrap();

        let state = sync(root.path(), &lockfile, &config, &[], None).unwrap();

        assert_eq!(state.linked, [format!(".claude/skills/{entry}")]);
        assert_eq!(
            link_target(&link),
            link_target(&root.path().join("node_modules/foo/skills/guide")),
        );
    }
}

#[test]
fn replaces_an_unrecorded_link_to_a_nested_skill_copy_in_the_virtual_store() {
    for old_slot in ["foo@1.0.0_peer@1.0.0", "foo@0.9.0"] {
        let (root, lockfile, config, entry) = workspace_with_peer_variant("engineering/implement");
        let old_skill = root
            .path()
            .join("node_modules/.pnpm")
            .join(old_slot)
            .join("node_modules/foo");
        install_package(&old_skill, &["engineering/implement"]);
        let link = root
            .path()
            .join(".claude/skills")
            .join(&entry);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(&old_skill.join("skills/engineering/implement"), &link).unwrap();

        let state = sync(root.path(), &lockfile, &config, &[], None).unwrap();

        assert_eq!(state.linked, [format!(".claude/skills/{entry}")]);
        assert_eq!(
            link_target(&link),
            link_target(&root.path().join("node_modules/foo/skills/engineering/implement")),
        );
    }
}

#[test]
fn unrecorded_links_to_another_skill_or_outside_store_fall_back_to_the_prefixed_name() {
    for other in [
        "node_modules/.pnpm/bar@1.0.0/node_modules/bar/skills/guide",
        "node_modules/.pnpm/foo@1.0.0_peer@1.0.0/node_modules/foo/skills/migrate",
        "node_modules/.pnpm/foo@1.0.0_peer@1.0.0/node_modules/foo/skills/engineering/migrate",
        "copies/node_modules/foo/skills/guide",
    ] {
        let (root, lockfile, config, bare) = workspace_with_peer_variant("guide");
        let target = root.path().join(other);
        fs::create_dir_all(&target).unwrap();
        let link = root
            .path()
            .join(".claude/skills")
            .join(&bare);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(&target, &link).unwrap();

        let state = sync(root.path(), &lockfile, &config, &[], None).unwrap();

        assert_eq!(state.linked, [".claude/skills/pnpm-foo-guide"], "{other}: {state:?}");
        assert_eq!(link_target(&link), link_target(&target));
    }
}

#[test]
fn two_packages_ship_the_same_skill_name_prefixes_both() {
    let (root, lockfile) = workspace(&[("a", "1.0.0", &["guide"]), ("b", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("a", true), ("b", true)], None), &[], None)
        .unwrap();

    assert_eq!(state.linked, [".claude/skills/pnpm-a-guide", ".claude/skills/pnpm-b-guide"]);
    // Both prefixed entries resolve to their own package's skill.
    assert_eq!(
        link_target(&root.path().join(".claude/skills/pnpm-a-guide")),
        link_target(&root.path().join("node_modules/a/skills/guide")),
    );
    assert_eq!(
        link_target(&root.path().join(".claude/skills/pnpm-b-guide")),
        link_target(&root.path().join("node_modules/b/skills/guide")),
    );
}

#[test]
fn a_bare_name_taken_in_any_dir_is_prefixed_in_all_of_them() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    let cursor = root.path().join(".cursor/skills");
    fs::create_dir_all(&cursor).unwrap();
    fs::write(cursor.join("guide"), "an agent minding its own file\n").unwrap();

    let state = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    assert_eq!(state.linked, [".claude/skills/pnpm-foo-guide", ".cursor/skills/pnpm-foo-guide"]);
    assert_eq!(
        fs::read_to_string(cursor.join("guide")).unwrap(),
        "an agent minding its own file\n",
    );
}

#[test]
fn the_names_stay_across_syncs_of_the_same_state() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    fs::create_dir_all(root.path().join(".claude/skills")).unwrap();
    let first = sync(root.path(), &lockfile, &config(&[("foo", true)], None), &[], None).unwrap();

    let second =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &first.linked, None).unwrap();

    assert_eq!(second, first);
}

#[test]
fn entries_prune_and_relink_when_the_naming_changes() {
    let (root, lockfile) = workspace(&[("foo", "1.0.0", &["guide"])]);
    let skills = root.path().join(".claude/skills");
    fs::create_dir_all(&skills).unwrap();
    // An entry that a pnpm which prefixed every name linked relinks under
    // the bare name.
    pnpm_fs::force_symlink_dir(
        &root.path().join("node_modules/foo/skills/guide"),
        &skills.join("pnpm-foo-guide"),
    )
    .unwrap();
    let linked = [".claude/skills/pnpm-foo-guide".to_string()];

    let state =
        sync(root.path(), &lockfile, &config(&[("foo", true)], None), &linked, None).unwrap();

    assert_eq!(state.linked, [".claude/skills/guide"]);
    assert!(fs::symlink_metadata(skills.join("pnpm-foo-guide")).is_err());
    assert!(skills.join("guide/SKILL.md").is_file());
}
