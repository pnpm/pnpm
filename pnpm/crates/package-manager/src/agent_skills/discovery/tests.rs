use super::{Skill, SkillSource, keep_highest};
use crate::agent_skills::{SyncAgentSkills, sync_agent_skills};
use pnpm_config::{Config, SkillsPermission};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use std::{collections::BTreeMap, path::PathBuf};

fn top_level() -> Skill {
    Skill { name: "guide".to_string(), dir: PathBuf::from("guide") }
}

fn source(package_dir: &str) -> SkillSource {
    SkillSource {
        dep_path: "foo@1.0.0".to_string(),
        approval_key: "foo".to_string(),
        link_segment: "foo".to_string(),
        package_dir: PathBuf::from(package_dir),
        skills: vec![top_level()],
    }
}

#[test]
fn equal_versions_choose_the_same_directory_in_either_order() {
    for directories in [["a", "b"], ["b", "a"]] {
        let mut selected = BTreeMap::new();
        for directory in directories {
            keep_highest(&mut selected, source(directory));
        }
        assert_eq!(selected["foo"].package_dir, PathBuf::from("a"));
    }
}

#[test]
fn peer_variants_have_distinct_probe_keys() {
    assert_ne!(
        super::probe_key("a", "foo@1.0.0(peer@1.0.0)"),
        super::probe_key("b", "foo@1.0.0(peer@2.0.0)"),
    );
}

#[test]
fn unresolved_package_locations_do_not_hide_skills_from_installed_variants() {
    let root = tempfile::tempdir().unwrap();
    let dangling = root.path().join("dangling");
    pnpm_fs::symlink_dir(&root.path().join("missing"), &dangling).unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "package").unwrap();
    let package = root.path().join("installed");
    let skill = package.join("skills/guide");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "guide").unwrap();

    for unresolved in [dangling, file] {
        let mut cache = std::collections::HashMap::new();
        assert!(super::cached_skills(&mut cache, "foo@1.0.0".to_string(), &unresolved).is_none());

        let (skills, package_dir) =
            super::cached_skills(&mut cache, "foo@1.0.0".to_string(), &package).unwrap();
        assert_eq!(skills, [top_level()]);
        assert_eq!(package_dir, std::fs::canonicalize(&package).unwrap());
    }
}

#[test]
fn peer_variants_without_skills_do_not_suppress_valid_skill_sources() {
    for directories in [["a", "b"], ["b", "a"]] {
        let mut selected = BTreeMap::new();
        for directory in directories {
            let mut variant = source(directory);
            if directory == "a" {
                variant.skills.clear();
            }
            keep_highest(&mut selected, variant);
        }
        assert_eq!(selected["foo"].package_dir, PathBuf::from("b"));
    }
}

#[test]
fn category_directories_hold_skills() {
    let root = tempfile::tempdir().unwrap();
    for relative in ["guide", "engineering/implement", "engineering/tooling/migrate"] {
        let dir = root
            .path()
            .join("skills")
            .join(relative);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "# skill\n").unwrap();
    }
    std::fs::write(root.path().join("skills/policy.md"), "not a skill\n").unwrap();
    let notes = root.path().join("skills/notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("README.md"), "no SKILL.md\n").unwrap();

    let found = super::skills(root.path());

    assert_eq!(
        found,
        [
            Skill { name: "implement".to_string(), dir: PathBuf::from("engineering/implement") },
            Skill {
                name: "migrate".to_string(),
                dir: PathBuf::from("engineering/tooling/migrate"),
            },
            Skill { name: "guide".to_string(), dir: PathBuf::from("guide") },
        ],
    );
}

#[test]
fn a_skill_name_shipped_in_two_categories_is_kept_once() {
    let root = tempfile::tempdir().unwrap();
    for relative in ["backend/guide", "frontend/Guide"] {
        let dir = root
            .path()
            .join("skills")
            .join(relative);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "# skill\n").unwrap();
    }

    let super::Probe::Found(Some((shipped, _))) = super::probe_package(root.path()) else {
        panic!("expected Probe::Found");
    };

    assert_eq!(shipped, [Skill { name: "guide".to_string(), dir: PathBuf::from("backend/guide") }]);
}

#[test]
fn a_git_ssh_package_is_named_up_to_its_first_at() {
    for (dep_path, segment) in [
        ("foo@git+ssh://git@github.com/org/foo.git#abc123", "foo"),
        ("@scope/foo@git+ssh://git@github.com/org/foo.git#abc123", "@scope+foo"),
        ("ä@git+ssh://git@github.com/org/foo.git#abc123", "ä"),
        ("ä@1.0.0", "ä"),
    ] {
        let source = super::skill_source((Vec::new(), PathBuf::from("pkg")), dep_path, "alias");
        assert_eq!(source.link_segment, segment, "{dep_path}");
    }
}

#[test]
fn non_package_name_prefixes_fall_back_to_alias() {
    for dep_path in [
        r"ä\..\..\outside@1.0.0",
        r"a\..\..\outside@1.0.0",
        "../outside@1.0.0",
        "../outside@git+ssh://git@github.com/org/foo.git#abc123",
        r"@scope/..\outside@1.0.0",
    ] {
        let source =
            super::skill_source((Vec::new(), PathBuf::from("pkg")), dep_path, "safe-alias");
        assert_eq!(source.link_segment, "safe-alias", "{dep_path}");
    }
}

#[test]
fn sync_does_not_escape_skills_dir_on_backslash_traversal_dep_path() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("workspace");
    let skill_dir = root.join("node_modules/safe-alias/skills/guide");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# guide\n").unwrap();
    let skills_dir = root.join(".claude/skills");
    std::fs::create_dir_all(skills_dir.join("guide")).unwrap();
    std::fs::create_dir_all(skills_dir.join("pnpm-ä")).unwrap();

    let yaml = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      safe-alias:\n        specifier: npm:foo@1.0.0\n        version: 'ä\\..\\..\\outside@1.0.0'\n";
    let mut config = Config {
        modules_dir: root.join("node_modules"),
        install_state_dir: root.join("node_modules/.pnpm"),
        skills_dirs: Some(vec![".claude/skills".to_string()]),
        ..Config::default()
    };
    config.allow_skills.insert(r"ä\..\..\outside".to_string(), SkillsPermission::All(true));

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: &root,
        lockfile: &serde_saphyr::from_str::<Lockfile>(yaml).unwrap(),
        included: IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: true,
        },
        linked: &[],
        agent_dir: None,
        hoisted_locations: None,
    })
    .unwrap();

    assert_eq!(state.linked, [".claude/skills/pnpm-safe-alias-guide"]);
    assert!(skills_dir.join("pnpm-safe-alias-guide").exists());
    assert!(!root.join(".claude/outside-guide").exists());
    assert!(
        !workspace
            .path()
            .join("outside-guide")
            .exists(),
    );
}

#[test]
fn a_skill_deeper_than_the_bound_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    let deep = root.path().join("skills/engineering/platform/agent-schema/conventions");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("SKILL.md"), "# skill\n").unwrap();

    let found = super::skills(root.path());

    assert_eq!(found, []);
}

#[test]
fn a_skill_directory_is_not_searched_for_inner_skills() {
    let root = tempfile::tempdir().unwrap();
    let guide = root.path().join("skills/guide");
    std::fs::create_dir_all(&guide).unwrap();
    std::fs::write(guide.join("SKILL.md"), "# skill\n").unwrap();
    let inner = guide.join("reference/conventions");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("SKILL.md"), "# skill\n").unwrap();

    let found = super::skills(root.path());

    assert_eq!(found, [top_level()]);
}

#[cfg(unix)]
#[test]
fn a_symlinked_category_directory_is_not_searched() {
    let root = tempfile::tempdir().unwrap();
    let guide = root.path().join("skills/guide");
    std::fs::create_dir_all(&guide).unwrap();
    std::fs::write(guide.join("SKILL.md"), "# skill\n").unwrap();
    std::os::unix::fs::symlink(root.path().join("skills"), root.path().join("skills/loop"))
        .unwrap();

    let found = super::skills(root.path());

    assert_eq!(found, [top_level()]);
}

#[cfg(unix)]
#[test]
fn a_symlinked_skill_directory_is_found() {
    let root = tempfile::tempdir().unwrap();
    let actual_skill = root.path().join("internal/guide");
    std::fs::create_dir_all(&actual_skill).unwrap();
    std::fs::write(actual_skill.join("SKILL.md"), "# skill\n").unwrap();
    let skills_dir = root.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::os::unix::fs::symlink(&actual_skill, skills_dir.join("guide")).unwrap();

    let found = super::skills(root.path());

    assert_eq!(found, [top_level()]);
}

#[cfg(unix)]
#[test]
fn a_symlink_escaping_package_is_dropped() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("SKILL.md"), "# skill\n").unwrap();

    let package = temp.path().join("package");
    let skills_dir = package.join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::os::unix::fs::symlink(&outside, skills_dir.join("escaped")).unwrap();

    let probe = super::probe_package(&package);
    let super::Probe::Found(Some((shipped, _))) = probe else {
        panic!("expected Probe::Found");
    };
    assert!(shipped.is_empty());
}

#[cfg(unix)]
#[test]
fn an_escaping_skill_does_not_hide_a_later_one_of_its_name() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("SKILL.md"), "# skill\n").unwrap();
    let package = temp.path().join("package");
    std::fs::create_dir_all(package.join("skills/a")).unwrap();
    std::os::unix::fs::symlink(&outside, package.join("skills/a/guide")).unwrap();
    std::fs::create_dir_all(package.join("skills/b/guide")).unwrap();
    std::fs::write(package.join("skills/b/guide/SKILL.md"), "# skill\n").unwrap();

    let super::Probe::Found(Some((shipped, _))) = super::probe_package(&package) else {
        panic!("expected Probe::Found");
    };

    assert_eq!(shipped, [Skill { name: "guide".to_string(), dir: PathBuf::from("b/guide") }]);
}
