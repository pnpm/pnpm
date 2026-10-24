use super::{SkillSource, keep_highest};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

fn source(package_dir: &str) -> SkillSource {
    SkillSource {
        dep_path: "foo@1.0.0".to_string(),
        approval_key: "foo".to_string(),
        link_segment: "foo".to_string(),
        package_dir: PathBuf::from(package_dir),
        skills: vec!["guide".to_string()],
        equivalent_skills: BTreeMap::new(),
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
fn probing_the_same_version_preserves_each_installed_directory() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = std::collections::HashMap::new();
    for name in ["b", "a"] {
        let package = root.path().join(name);
        let skill = package.join("skills/guide");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "guide").unwrap();
        let (_, found) =
            super::cached_skills(&mut cache, "foo@1.0.0".to_string(), &package).unwrap();
        assert_eq!(found, std::fs::canonicalize(package).unwrap());
    }
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
        let (empty_skills, unresolved_dir) =
            super::cached_skills(&mut cache, "foo@1.0.0".to_string(), &unresolved).unwrap();
        assert!(empty_skills.is_empty());
        assert_eq!(unresolved_dir, unresolved);

        let (skills, package_dir) =
            super::cached_skills(&mut cache, "foo@1.0.0".to_string(), &package).unwrap();
        assert_eq!(skills, ["guide"]);
        assert_eq!(package_dir, std::fs::canonicalize(&package).unwrap());
    }
}

#[test]
fn merging_variants_keeps_valid_alternatives_without_adding_skills() {
    let mut selected = BTreeMap::new();
    let mut without_skills = source("b");
    without_skills.skills.clear();
    for variant in [source("c"), without_skills, source("a")] {
        keep_highest(&mut selected, variant);
    }
    assert_eq!(selected["foo"].skills, ["guide"]);
    assert_eq!(
        selected["foo"].equivalent_skills["guide"],
        BTreeSet::from([PathBuf::from("c/skills/guide")]),
    );
    keep_highest(&mut selected, {
        let mut variant = source("0");
        variant.skills.clear();
        variant
    });
    assert_eq!(selected["foo"].skills, Vec::<String>::new());
}
