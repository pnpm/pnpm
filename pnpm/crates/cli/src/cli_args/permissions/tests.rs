use super::{
    ApproveArgs, Capabilities,
    decisions::{PackageDecisions, confirmation},
    render_permissions,
};
use pnpm_config::{
    Config,
    PermissionCapability::{Build, Skills},
    SkillsPermission,
};
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{collections::BTreeMap, fs, path::Path};
use tempfile::TempDir;

fn pending(
    entries: &[(&str, &[pnpm_config::PermissionCapability])],
) -> BTreeMap<String, Capabilities> {
    entries
        .iter()
        .map(|(pkg, capabilities)| (pkg.to_string(), capabilities.iter().copied().collect()))
        .collect()
}

fn args(packages: &[&str], all: bool) -> ApproveArgs {
    ApproveArgs {
        packages: packages
            .iter()
            .map(ToString::to_string)
            .collect(),
        all,
    }
}

fn decided(build: Option<bool>, skills: Option<SkillsPermission>) -> PackageDecisions {
    PackageDecisions { build, skills }
}

/// A config and manifest dir the named-approval and `--all` routes never
/// read: they decide without discovering skills.
fn decide_config() -> &'static Config {
    Box::leak(Box::new(Config::default()))
}

#[test]
fn a_named_package_is_decided_for_everything_it_requests() {
    let pending = pending(&[("drizzle-kit", &[Build, Skills]), ("esbuild", &[Build])]);

    let approval = args(&["drizzle-kit", "!esbuild"], false)
        .decide::<SilentReporter>(&pending, decide_config(), Path::new("/workspace"))
        .unwrap()
        .unwrap();

    assert_eq!(
        approval.decisions,
        BTreeMap::from([
            ("drizzle-kit".to_string(), decided(Some(true), Some(SkillsPermission::All(true))),),
            ("esbuild".to_string(), decided(Some(false), None)),
        ]),
    );
}

#[test]
fn a_package_named_before_it_is_pending_is_decided_for_every_capability() {
    let approval = args(&["!foo"], false)
        .decide::<SilentReporter>(&BTreeMap::new(), decide_config(), Path::new("/workspace"))
        .unwrap()
        .unwrap();

    assert_eq!(
        approval.decisions,
        BTreeMap::from([(
            "foo".to_string(),
            decided(Some(false), Some(SkillsPermission::All(false))),
        )]),
    );
}

#[test]
fn all_approves_every_pending_capability() {
    let pending = pending(&[("drizzle-kit", &[Skills]), ("esbuild", &[Build])]);

    let approval = args(&[], true)
        .decide::<SilentReporter>(&pending, decide_config(), Path::new("/workspace"))
        .unwrap()
        .unwrap();

    assert_eq!(
        approval.decisions,
        BTreeMap::from([
            ("drizzle-kit".to_string(), decided(None, Some(SkillsPermission::All(true))),),
            ("esbuild".to_string(), decided(Some(true), None)),
        ]),
    );
}

#[test]
fn rejects_contradictory_arguments() {
    let error = args(&["foo", "!foo"], false)
        .decide::<SilentReporter>(&BTreeMap::new(), decide_config(), Path::new("/workspace"))
        .expect_err("contradiction");
    assert!(error.to_string().contains("both approved and denied"));
}

#[test]
fn lists_the_decisions_by_package() {
    let modules_dir = tempfile::tempdir().unwrap();
    let config = Config {
        modules_dir: modules_dir.path().to_path_buf(),
        allow_builds: [
            ("esbuild".to_string(), true),
            ("drizzle-kit".to_string(), true),
            ("sharp".to_string(), false),
        ]
        .into(),
        allow_skills: [("drizzle-kit".to_string(), SkillsPermission::All(true))].into(),
        ..Config::default()
    };

    assert_eq!(
        render_permissions(&config).unwrap(),
        "Granted:\n  drizzle-kit  build, skills\n  esbuild      build\n\nDenied:\n  sharp  build\n",
    );
}

#[test]
fn lists_chosen_skills_next_to_the_package() {
    let modules_dir = tempfile::tempdir().unwrap();
    let config = Config {
        modules_dir: modules_dir.path().to_path_buf(),
        allow_skills: [
            ("drizzle-kit".to_string(), SkillsPermission::Chosen(vec!["guide".to_string()])),
            ("esbuild".to_string(), SkillsPermission::All(false)),
        ]
        .into(),
        ..Config::default()
    };

    assert_eq!(
        render_permissions(&config).unwrap(),
        "Granted:\n  drizzle-kit  skills (guide)\n\nDenied:\n  esbuild  skills\n",
    );
}

#[test]
fn the_confirmation_names_packages_without_their_dep_path() {
    let pending =
        pending(&[("@pnpm/marked-skills", &[Skills]), ("@pnpm/git-skills", &[Build, Skills])]);

    assert_eq!(
        confirmation(&pending.keys().cloned().collect(), &pending),
        "Approve @pnpm/git-skills (build, skills), @pnpm/marked-skills (skills)?",
    );
}

/// A config whose `.modules.yaml` awaits the approval of `pending_skills`.
fn config_awaiting(dir: &TempDir, pending_skills: &[&str]) -> Config {
    let modules_dir = dir.path().join("node_modules");
    fs::create_dir_all(&modules_dir).unwrap();
    let manifest = serde_json::json!({
        "layoutVersion": 5,
        "packageManager": "pacquet@test",
        "storeDir": "/store",
        "virtualStoreDir": ".pnpm",
        "pendingSkills": pending_skills,
    });
    fs::write(modules_dir.join(".modules.yaml"), manifest.to_string()).unwrap();
    Config { modules_dir, ..Config::new() }
}

#[test]
fn the_list_shows_a_git_package_by_short_label() {
    let dir = tempfile::tempdir().unwrap();
    let config = config_awaiting(
        &dir,
        &["mattpocock-skills@https://codeload.github.com/mattpocock/skills/tar.gz/a496f551"],
    );

    assert_eq!(
        render_permissions(&config).unwrap(),
        "Awaiting approval:\n  mattpocock-skills (git)  skills\n\nRun \"pnpm approve\" to review the packages awaiting approval.\n",
    );
}
