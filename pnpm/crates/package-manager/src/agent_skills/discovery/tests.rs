use super::{Skill, SkillSource, keep_highest};
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
