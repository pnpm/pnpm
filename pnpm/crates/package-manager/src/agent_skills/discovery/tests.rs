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

#[test]
fn local_directory_and_tarball_dep_paths_use_the_project_alias_for_link_segment() {
    for dep_path in [
        "@example/my-skills@link:packages/my-skills",
        "@example/my-skills@file:../my-skills",
        "@example/my-skills@file:./my-skills.tgz",
        "@example/my-skills@https://example.com/my-skills.tgz",
    ] {
        let source =
            super::skill_source((Vec::new(), PathBuf::from("pkg")), dep_path, "@app/custom-alias");
        assert_eq!(source.link_segment, "@app+custom-alias", "{dep_path}");
    }
}

#[test]
fn unsafe_importer_ids_are_ignored_during_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let outside_pkg = temp.path().join("outside/node_modules/evil");
    std::fs::create_dir_all(outside_pkg.join("skills/pwn")).unwrap();
    std::fs::write(outside_pkg.join("skills/pwn/SKILL.md"), "# pwn\n").unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join(".agents/skills")).unwrap();

    let yaml = "lockfileVersion: '9.0'\nimporters:\n  '../outside':\n    dependencies:\n      evil:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    let mut config = Config {
        modules_dir: root.join("node_modules"),
        install_state_dir: root.join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert("evil".to_string(), SkillsPermission::All(true));

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

    assert!(state.linked.is_empty());
    assert!(state.pending.is_empty());
}

#[test]
fn hoisted_workspace_link_without_hoisted_locations_discovers_and_prunes_skills() {
    let root = tempfile::tempdir().unwrap();
    let pkg_dir = root.path().join("packages/my-skills");
    std::fs::create_dir_all(pkg_dir.join("skills/demo-skill")).unwrap();
    std::fs::write(pkg_dir.join("package.json"), r#"{"name":"@example/my-skills"}"#).unwrap();
    std::fs::write(pkg_dir.join("skills/demo-skill/SKILL.md"), "# demo\n").unwrap();

    let app_link = root.path().join("packages/app/node_modules/@example/my-skills");
    std::fs::create_dir_all(app_link.parent().unwrap()).unwrap();
    pnpm_fs::force_symlink_dir(&pkg_dir, &app_link).unwrap();
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let yaml = "lockfileVersion: '9.0'\nimporters:\n  packages/app:\n    dependencies:\n      '@example/my-skills':\n        specifier: workspace:*\n        version: link:../my-skills\n";
    let lockfile = serde_saphyr::from_str::<Lockfile>(yaml).unwrap();
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };
    let mut config = Config {
        node_linker: pnpm_config::NodeLinker::Hoisted,
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };

    let unapproved = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &lockfile,
        included,
        linked: &[],
        agent_dir: None,
        hoisted_locations: None,
    })
    .unwrap();
    assert_eq!(unapproved.pending, ["@example/my-skills@link:packages/my-skills"]);
    assert_eq!(unapproved.pending_keys(), ["@example/my-skills"]);

    config.allow_skills.insert("@example/my-skills".to_string(), SkillsPermission::All(true));
    let approved = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &lockfile,
        included,
        linked: &[],
        agent_dir: None,
        hoisted_locations: None,
    })
    .unwrap();
    assert_eq!(approved.linked, [".agents/skills/demo-skill"]);
    assert!(
        root.path()
            .join(".agents/skills/demo-skill/SKILL.md")
            .is_file(),
    );

    config.allow_skills.insert("@example/my-skills".to_string(), SkillsPermission::All(false));
    let revoked = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &lockfile,
        included,
        linked: &approved.linked,
        agent_dir: None,
        hoisted_locations: None,
    })
    .unwrap();
    assert!(revoked.linked.is_empty());
    assert!(
        !root
            .path()
            .join(".agents/skills/demo-skill")
            .exists(),
    );
}

#[test]
fn workspace_link_and_injected_copy_collapse_to_live_link_in_either_importer_order() {
    let root = tempfile::tempdir().unwrap();
    let live_dir = root.path().join("packages/my-skills");
    std::fs::create_dir_all(live_dir.join("skills/demo-skill")).unwrap();
    std::fs::write(live_dir.join("package.json"), r#"{"name":"@example/my-skills"}"#).unwrap();
    std::fs::write(live_dir.join("skills/demo-skill/SKILL.md"), "# live\n").unwrap();

    let injected_dir = root
        .path()
        .join("node_modules/.pnpm/@example+my-skills@file+packages+my-skills/node_modules/@example/my-skills");
    std::fs::create_dir_all(injected_dir.join("skills/demo-skill")).unwrap();
    std::fs::write(injected_dir.join("package.json"), r#"{"name":"@example/my-skills"}"#).unwrap();
    std::fs::write(injected_dir.join("skills/demo-skill/SKILL.md"), "# injected\n").unwrap();
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert("@example/my-skills".to_string(), SkillsPermission::All(true));
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };

    for (link_importer, injected_importer) in
        [("packages/a", "packages/b"), ("packages/b", "packages/a")]
    {
        for dir in ["packages/a/node_modules", "packages/b/node_modules"] {
            let _ = std::fs::remove_dir_all(root.path().join(dir));
        }
        let link_slot = root
            .path()
            .join(link_importer)
            .join("node_modules/@example/my-skills");
        std::fs::create_dir_all(link_slot.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(&live_dir, &link_slot).unwrap();

        let injected_slot = root
            .path()
            .join(injected_importer)
            .join("node_modules/@example/my-skills");
        std::fs::create_dir_all(injected_slot.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(&injected_dir, &injected_slot).unwrap();

        let yaml = format!(
            "lockfileVersion: '9.0'\nimporters:\n  {link_importer}:\n    dependencies:\n      '@example/my-skills':\n        specifier: workspace:*\n        version: link:../my-skills\n  {injected_importer}:\n    dependencies:\n      '@example/my-skills':\n        specifier: workspace:*\n        version: file:packages/my-skills\n",
        );
        let state = sync_agent_skills(&SyncAgentSkills {
            config: &config,
            workspace_root: root.path(),
            lockfile: &serde_saphyr::from_str::<Lockfile>(&yaml).unwrap(),
            included,
            linked: &[".agents/skills/demo-skill".to_string()],
            agent_dir: None,
            hoisted_locations: None,
        })
        .unwrap();

        assert_eq!(state.linked, [".agents/skills/demo-skill"]);
        assert_eq!(
            std::fs::canonicalize(root.path().join(".agents/skills/demo-skill")).unwrap(),
            std::fs::canonicalize(live_dir.join("skills/demo-skill")).unwrap(),
        );
    }
}

#[test]
fn distinct_local_and_registry_sources_sharing_alias_and_skill_disambiguate_without_collision() {
    let root = tempfile::tempdir().unwrap();
    let dir_a = root.path().join("dir-a");
    let dir_b = root.path().join("dir-b");
    let reg_c = root.path().join("packages/c/node_modules/foo");
    for dir in [&dir_a, &dir_b, &reg_c] {
        std::fs::create_dir_all(dir.join("skills/guide")).unwrap();
        std::fs::write(dir.join("skills/guide/SKILL.md"), "# guide\n").unwrap();
    }
    for (imp, target) in [("packages/a", &dir_a), ("packages/b", &dir_b)] {
        let slot = root
            .path()
            .join(imp)
            .join("node_modules/foo");
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(target, &slot).unwrap();
    }
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let yaml = concat!(
        "lockfileVersion: '9.0'\nimporters:\n",
        "  packages/a:\n    dependencies:\n      foo:\n        specifier: link:../../dir-a\n        version: link:../../dir-a\n",
        "  packages/b:\n    dependencies:\n      foo:\n        specifier: link:../../dir-b\n        version: link:../../dir-b\n",
        "  packages/c:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0\n",
    );
    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert("foo".to_string(), SkillsPermission::All(true));

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
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

    assert_eq!(
        state.linked,
        [
            ".agents/skills/pnpm-foo-dir-a-guide",
            ".agents/skills/pnpm-foo-dir-b-guide",
            ".agents/skills/pnpm-foo-guide",
        ],
    );
    assert_eq!(
        std::fs::canonicalize(root.path().join(".agents/skills/pnpm-foo-guide")).unwrap(),
        std::fs::canonicalize(reg_c.join("skills/guide")).unwrap(),
    );
    assert_eq!(
        std::fs::canonicalize(root.path().join(".agents/skills/pnpm-foo-dir-a-guide")).unwrap(),
        std::fs::canonicalize(dir_a.join("skills/guide")).unwrap(),
    );
    assert_eq!(
        std::fs::canonicalize(root.path().join(".agents/skills/pnpm-foo-dir-b-guide")).unwrap(),
        std::fs::canonicalize(dir_b.join("skills/guide")).unwrap(),
    );
}

#[test]
fn explicit_injected_file_approval_links_when_live_link_copy_is_undecided() {
    let root = tempfile::tempdir().unwrap();
    let live_dir = root.path().join("packages/my-skills");
    let injected_dir =
        root.path().join("node_modules/.pnpm/injected/node_modules/@example/my-skills");
    for dir in [&live_dir, &injected_dir] {
        std::fs::create_dir_all(dir.join("skills/demo-skill")).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"name": "@example/my-skills", "version": "1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("skills/demo-skill/SKILL.md"), "# demo\n").unwrap();
    }
    let link_slot = root.path().join("packages/a/node_modules/@example/my-skills");
    std::fs::create_dir_all(link_slot.parent().unwrap()).unwrap();
    pnpm_fs::force_symlink_dir(&live_dir, &link_slot).unwrap();

    let injected_slot = root.path().join("packages/b/node_modules/@example/my-skills");
    std::fs::create_dir_all(injected_slot.parent().unwrap()).unwrap();
    pnpm_fs::force_symlink_dir(&injected_dir, &injected_slot).unwrap();
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let yaml = concat!(
        "lockfileVersion: '9.0'\nimporters:\n",
        "  packages/a:\n    dependencies:\n      '@example/my-skills':\n",
        "        specifier: workspace:*\n        version: link:../my-skills\n",
        "  packages/b:\n    dependencies:\n      '@example/my-skills':\n",
        "        specifier: workspace:*\n        version: file:packages/my-skills\n",
    );
    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert(
        "@example/my-skills@file:packages/my-skills".to_string(),
        SkillsPermission::All(true),
    );

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
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

    assert_eq!(state.linked, [".agents/skills/demo-skill"]);
    assert_eq!(state.pending, ["@example/my-skills@link:packages/my-skills"]);
    assert_eq!(
        std::fs::canonicalize(root.path().join(".agents/skills/demo-skill")).unwrap(),
        std::fs::canonicalize(injected_dir.join("skills/demo-skill")).unwrap(),
    );
}

#[test]
fn deep_local_paths_sharing_alias_and_skill_use_bounded_hash_filenames() {
    let root = tempfile::tempdir().unwrap();
    let seg = "nested-segment-with-twenty-eight-chars";
    let rel_a = format!("{seg}/{seg}/{seg}/{seg}/{seg}/alpha");
    let rel_b = format!("{seg}/{seg}/{seg}/{seg}/{seg}/bravo");
    let dir_a = root.path().join(&rel_a);
    let dir_b = root.path().join(&rel_b);
    for dir in [&dir_a, &dir_b] {
        std::fs::create_dir_all(dir.join("skills/guide")).unwrap();
        std::fs::write(dir.join("skills/guide/SKILL.md"), "# guide\n").unwrap();
    }
    let alias = "shared-thirty-character-alias0";
    for (imp, target) in [("packages/a", &dir_a), ("packages/b", &dir_b)] {
        let slot = root
            .path()
            .join(imp)
            .join("node_modules")
            .join(alias);
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(target, &slot).unwrap();
    }
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let yaml = format!(
        "lockfileVersion: '9.0'\nimporters:\n  packages/a:\n    dependencies:\n      {alias}:\n        specifier: link:../../{rel_a}\n        version: link:../../{rel_a}\n  packages/b:\n    dependencies:\n      {alias}:\n        specifier: link:../../{rel_b}\n        version: link:../../{rel_b}\n",
    );
    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert(alias.to_string(), SkillsPermission::All(true));

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &serde_saphyr::from_str::<Lockfile>(&yaml).unwrap(),
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

    assert_eq!(state.linked.len(), 2);
    let mut resolved = Vec::new();
    for entry in &state.linked {
        let path = root.path().join(entry);
        let name = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        assert!(name.len() <= 255, "filename exceeded 255 bytes: {name}");
        resolved.push(std::fs::canonicalize(path).unwrap());
    }
    resolved.sort();
    let mut expected = vec![
        std::fs::canonicalize(dir_a.join("skills/guide")).unwrap(),
        std::fs::canonicalize(dir_b.join("skills/guide")).unwrap(),
    ];
    expected.sort();
    assert_eq!(resolved, expected);
}

#[test]
fn revoking_descendant_link_package_inside_configured_skills_dir_prunes_symlink() {
    let root = tempfile::tempdir().unwrap();
    let pkg_dir = root.path().join("ai-skills/pkg");
    std::fs::create_dir_all(pkg_dir.join("skills/guide")).unwrap();
    std::fs::write(pkg_dir.join("package.json"), r#"{"name": "pkg", "version": "1.0.0"}"#).unwrap();
    std::fs::write(pkg_dir.join("skills/guide/SKILL.md"), "# guide\n").unwrap();

    let slot = root.path().join("node_modules/pkg");
    std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
    pnpm_fs::force_symlink_dir(&pkg_dir, &slot).unwrap();

    let lockfile = serde_saphyr::from_str::<Lockfile>(concat!(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n",
        "      pkg:\n        specifier: link:./ai-skills/pkg\n        version: link:ai-skills/pkg\n",
    ))
    .unwrap();
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };
    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        skills_dirs: Some(vec!["ai-skills".to_string()]),
        ..Config::default()
    };
    config.allow_skills.insert("pkg".to_string(), SkillsPermission::All(true));

    let linked = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &lockfile,
        included,
        linked: &[],
        agent_dir: None,
        hoisted_locations: None,
    })
    .unwrap();
    assert_eq!(linked.linked, ["ai-skills/guide"]);
    assert!(
        root.path()
            .join("ai-skills/guide/SKILL.md")
            .is_file(),
    );

    config.allow_skills.insert("pkg".to_string(), SkillsPermission::All(false));
    let revoked = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &lockfile,
        included,
        linked: &linked.linked,
        agent_dir: None,
        hoisted_locations: None,
    })
    .unwrap();
    assert!(revoked.linked.is_empty());
    assert!(std::fs::symlink_metadata(root.path().join("ai-skills/guide")).is_err());
}

#[test]
fn long_sibling_skill_names_do_not_erase_colliding_local_package_labels() {
    let root = tempfile::tempdir().unwrap();
    let seg = "nested-segment-with-twenty-eight-chars";
    let rel_a = format!("{seg}/{seg}/{seg}/{seg}/{seg}/alpha");
    let rel_b = format!("{seg}/{seg}/{seg}/{seg}/{seg}/bravo");
    let dir_a = root.path().join(&rel_a);
    let dir_b = root.path().join(&rel_b);
    let long_shared = format!("shared-{}", "s".repeat(242));
    let long_only_a = format!("only-a-{}", "a".repeat(242));
    let long_only_b = format!("only-b-{}", "b".repeat(242));
    for (dir, unique_long) in [(&dir_a, &long_only_a), (&dir_b, &long_only_b)] {
        for skill in ["guide", long_shared.as_str(), unique_long.as_str()] {
            std::fs::create_dir_all(dir.join("skills").join(skill)).unwrap();
            std::fs::write(
                dir.join("skills")
                    .join(skill)
                    .join("SKILL.md"),
                "# skill\n",
            )
            .unwrap();
        }
    }
    for (imp, target) in [("packages/a", &dir_a), ("packages/b", &dir_b)] {
        let slot = root
            .path()
            .join(imp)
            .join("node_modules/foo");
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(target, &slot).unwrap();
    }
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let yaml = format!(
        "lockfileVersion: '9.0'\nimporters:\n  packages/a:\n    dependencies:\n      foo:\n        specifier: link:../../{rel_a}\n        version: link:../../{rel_a}\n  packages/b:\n    dependencies:\n      foo:\n        specifier: link:../../{rel_b}\n        version: link:../../{rel_b}\n",
    );
    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert("foo".to_string(), SkillsPermission::All(true));

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
        lockfile: &serde_saphyr::from_str::<Lockfile>(&yaml).unwrap(),
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

    assert_eq!(state.linked.len(), 6);
    for entry in &state.linked {
        let name = root
            .path()
            .join(entry)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(name.len() <= 255, "filename exceeded 255 bytes: {name}");
        assert!(!name.starts_with("pnpm--"), "erased package label: {name}");
    }
}

#[test]
fn parenthesized_link_dir_approval_does_not_approve_sibling_link_dir() {
    let root = tempfile::tempdir().unwrap();
    let dir_plain = root.path().join("packages/foo");
    let dir_paren = root.path().join("packages/foo(alt)");
    for dir in [&dir_plain, &dir_paren] {
        std::fs::create_dir_all(dir.join("skills/guide")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"name": "foo", "version": "1.0.0"}"#).unwrap();
        std::fs::write(dir.join("skills/guide/SKILL.md"), "# guide\n").unwrap();
    }
    for (imp, target) in [("packages/a", &dir_plain), ("packages/b", &dir_paren)] {
        let slot = root
            .path()
            .join(imp)
            .join("node_modules/foo");
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        pnpm_fs::force_symlink_dir(target, &slot).unwrap();
    }
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();

    let yaml = concat!(
        "lockfileVersion: '9.0'\nimporters:\n",
        "  packages/a:\n    dependencies:\n      foo:\n        specifier: link:../foo\n        version: link:../foo\n",
        "  packages/b:\n    dependencies:\n      foo:\n        specifier: link:../foo(alt)\n        version: link:../foo(alt)\n",
    );
    let mut config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };
    config.allow_skills.insert(
        "foo@link:packages/foo(alt)".to_string(),
        SkillsPermission::All(true),
    );

    let state = sync_agent_skills(&SyncAgentSkills {
        config: &config,
        workspace_root: root.path(),
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

    assert_eq!(state.linked, [".agents/skills/guide"]);
    assert_eq!(state.pending, ["foo@link:packages/foo"]);
    assert_eq!(
        std::fs::canonicalize(root.path().join(".agents/skills/guide")).unwrap(),
        std::fs::canonicalize(dir_paren.join("skills/guide")).unwrap(),
    );
}
