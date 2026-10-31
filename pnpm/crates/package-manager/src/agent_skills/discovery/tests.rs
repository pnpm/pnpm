use super::{SkillSource, keep_highest};
use std::{collections::BTreeMap, path::PathBuf};

fn source(package_dir: &str) -> SkillSource {
    SkillSource {
        dep_path: "foo@1.0.0".to_string(),
        approval_key: "foo".to_string(),
        package_dir: PathBuf::from(package_dir),
        skills: vec!["guide".to_string()],
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
        assert_eq!(skills, ["guide"]);
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
