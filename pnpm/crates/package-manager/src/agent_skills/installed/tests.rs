use super::{
    SkillApprovalOptions, resync_agent_skills_at, resync_installed_agent_skills,
    skill_approval_options,
};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::{Host, IncludedDependencies, Modules, write_modules_manifest};
use pretty_assertions::assert_eq;
use std::{fmt::Write, fs};
use tempfile::TempDir;

const ALL_GROUPS: IncludedDependencies = IncludedDependencies {
    dependencies: true,
    dev_dependencies: true,
    optional_dependencies: true,
};

fn workspace_with_package(pkg: &str, version: &str, skills: &[&str]) -> (TempDir, Lockfile) {
    let root = tempfile::tempdir().expect("create workspace");
    let mut lockfile =
        String::from("lockfileVersion: '9.0'\nimporters:\n  .:\n    devDependencies:\n");
    writeln!(lockfile, "      '{pkg}':\n        specifier: {version}\n        version: {version}")
        .expect("write lockfile");
    let pkg_dir = root
        .path()
        .join("node_modules")
        .join(pkg);
    fs::create_dir_all(&pkg_dir).expect("create package dir");
    for skill in skills {
        let skill_dir = pkg_dir.join("skills").join(skill);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("SKILL.md"), "# skill\n").expect("write SKILL.md");
    }
    (root, serde_saphyr::from_str(&lockfile).expect("parse lockfile"))
}

#[test]
fn resync_and_options_do_nothing_when_no_modules_installed() {
    let root = tempfile::tempdir().expect("create workspace");
    let config = Config {
        modules_dir: root.path().join("node_modules"),
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };

    assert_eq!(resync_installed_agent_skills(&config, root.path()).unwrap(), ());
    assert_eq!(
        skill_approval_options(&config, root.path()).unwrap(),
        SkillApprovalOptions::default(),
    );
}

#[test]
fn resync_and_options_do_nothing_when_lockfile_absent() {
    let root = tempfile::tempdir().expect("create workspace");
    let modules_dir = root.path().join("node_modules");
    fs::create_dir_all(&modules_dir).expect("create modules dir");
    let modules = Modules::default();
    write_modules_manifest::<Host>(&modules_dir, modules).expect("write modules");

    let config = Config {
        modules_dir,
        install_state_dir: root.path().join("node_modules/.pnpm"),
        ..Config::default()
    };

    assert_eq!(resync_installed_agent_skills(&config, root.path()).unwrap(), ());
    assert_eq!(
        skill_approval_options(&config, root.path()).unwrap(),
        SkillApprovalOptions::default(),
    );
}

#[test]
fn discovers_options_and_resyncs_installed_skills() {
    let (root, lockfile) = workspace_with_package("drizzle-kit", "1.0.0", &["guide", "migrate"]);
    let modules_dir = root.path().join("node_modules");
    let state_dir = modules_dir.join(".pnpm");
    fs::create_dir_all(&state_dir).expect("create state dir");
    lockfile.save_current_to_install_state_dir(&state_dir).expect("save lockfile");

    let modules = Modules { included: ALL_GROUPS, ..Modules::default() };
    write_modules_manifest::<Host>(&modules_dir, modules).expect("write modules");

    let config = Config {
        workspace_dir: Some(root.path().to_path_buf()),
        modules_dir,
        install_state_dir: state_dir,
        ..Config::default()
    };

    let options = skill_approval_options(&config, root.path()).expect("options");
    assert_eq!(
        options.skills_by_key.get("drizzle-kit"),
        Some(&vec!["guide".to_string(), "migrate".to_string()]),
    );
    assert!(!options.detected_dirs.is_empty());

    let pending = resync_agent_skills_at(&config, root.path(), Some(&lockfile)).expect("resync");
    assert_eq!(pending, vec!["drizzle-kit".to_string()]);

    let pending_again =
        resync_agent_skills_at(&config, root.path(), Some(&lockfile)).expect("resync again");
    assert_eq!(pending_again, vec!["drizzle-kit".to_string()]);

    resync_installed_agent_skills(&config, root.path()).expect("resync installed");
}
