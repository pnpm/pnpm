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

#[test]
fn the_list_keeps_pending_packages_with_one_short_label_apart() {
    let dir = tempfile::tempdir().unwrap();
    let config = config_awaiting(
        &dir,
        &[
            "foo@https://codeload.github.com/org1/foo/tar.gz/1",
            "foo@https://codeload.github.com/org2/foo/tar.gz/2",
        ],
    );

    let listing = render_permissions(&config).unwrap();

    assert!(
        listing.contains("foo (git) [foo@https://codeload.github.com/org1/foo/tar.gz/1]"),
        "{listing}",
    );
    assert!(
        listing.contains("foo (git) [foo@https://codeload.github.com/org2/foo/tar.gz/2]"),
        "{listing}",
    );
}

#[test]
fn the_list_labels_a_short_label_apart_across_sections() {
    let modules_dir = tempfile::tempdir().unwrap();
    let config = Config {
        modules_dir: modules_dir.path().to_path_buf(),
        allow_builds: [
            ("foo@https://codeload.github.com/org1/foo/tar.gz/1".to_string(), true),
            ("foo@https://codeload.github.com/org2/foo/tar.gz/2".to_string(), false),
        ]
        .into(),
        ..Config::default()
    };

    assert_eq!(
        render_permissions(&config).unwrap(),
        "Granted:\n  foo (git) [foo@https://codeload.github.com/org1/foo/tar.gz/1]  build\n\nDenied:\n  foo (git) [foo@https://codeload.github.com/org2/foo/tar.gz/2]  build\n",
    );
}

#[tokio::test]
async fn approve_run_with_no_pending_and_no_packages_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let config =
        Config::leak(Config { modules_dir: dir.path().join("node_modules"), ..Config::new() });
    let manifest = dir.path().join("package.json");
    fs::write(&manifest, "{}\n").unwrap();

    let result = args(&[], false).run::<SilentReporter>(dir.path(), config, &manifest).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn approve_run_denying_package_writes_decisions_and_exits() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_manifest = dir.path().join("pnpm-workspace.yaml");
    fs::write(&workspace_manifest, "packages:\n  - .\n").unwrap();
    let config = Config::leak(Config {
        workspace_dir: Some(dir.path().to_path_buf()),
        modules_dir: dir.path().join("node_modules"),
        ..Config::new()
    });
    let manifest = dir.path().join("package.json");
    fs::write(&manifest, "{}\n").unwrap();

    let result = args(&["!foo"], false).run::<SilentReporter>(dir.path(), config, &manifest).await;
    assert!(result.is_ok());
    let written = fs::read_to_string(&workspace_manifest).unwrap();
    assert!(written.contains("foo"));
}

#[test]
fn pending_builds_approved_filters_only_approved_build_requests() {
    let mut decisions = BTreeMap::new();
    decisions.insert("foo".to_string(), decided(Some(true), None));
    decisions.insert("bar".to_string(), decided(Some(false), None));
    decisions.insert("baz".to_string(), decided(Some(true), None));

    let pending = pending(&[("foo", &[Build]), ("bar", &[Build])]);
    let approved = super::decisions::pending_builds_approved(&decisions, &pending);
    assert_eq!(approved, vec!["foo".to_string()]);
}

#[test]
fn prompt_for_package_skills_with_empty_skills_is_undiscovered() {
    let outcome = super::prompt_for_package_skills("foo", &[]).unwrap();
    assert_eq!(outcome, super::SkillPromptOutcome::Undiscovered);
}

#[test]
fn a_single_skill_is_approved_with_its_package_without_a_prompt() {
    let outcome = super::prompt_for_package_skills("foo", &["guide".to_string()]).unwrap();
    assert_eq!(outcome, super::SkillPromptOutcome::Decided(SkillsPermission::All(true)));
}

#[test]
fn review_skills_with_undiscovered_skills_leaves_package_out_of_review() {
    let approved = std::collections::BTreeSet::from(["foo".to_string()]);
    let pending = pending(&[("foo", &[Skills])]);
    let review =
        super::review_skills(decide_config(), Path::new("/workspace"), &approved, &pending)
            .unwrap()
            .unwrap();
    assert!(review.skills_by_package.is_empty());
}

#[test]
fn decisions_by_approval_leaves_undiscovered_skills_undecided_for_approved_package() {
    let approved = std::collections::BTreeSet::from(["foo".to_string()]);
    let pending = pending(&[("foo", &[Build, Skills]), ("bar", &[Skills])]);
    let decisions = super::decisions::decisions_by_approval(&approved, &pending, &BTreeMap::new());
    assert_eq!(
        decisions,
        BTreeMap::from([
            ("bar".to_string(), decided(None, Some(SkillsPermission::All(false)))),
            ("foo".to_string(), decided(Some(true), None)),
        ]),
    );
}

#[test]
fn package_choices_distinguishes_colliding_labels() {
    let pending = pending(&[
        ("foo@https://codeload.github.com/org1/foo/tar.gz/1", &[Build]),
        ("foo@https://codeload.github.com/org2/foo/tar.gz/2", &[Skills]),
        ("bar", &[Build, Skills]),
    ]);
    let choices = super::package_choices(&pending);
    assert_eq!(
        choices,
        vec![
            ("bar".to_string(), "bar  build, skills".to_string()),
            (
                "foo@https://codeload.github.com/org1/foo/tar.gz/1".to_string(),
                "foo (git) [foo@https://codeload.github.com/org1/foo/tar.gz/1]  build".to_string(),
            ),
            (
                "foo@https://codeload.github.com/org2/foo/tar.gz/2".to_string(),
                "foo (git) [foo@https://codeload.github.com/org2/foo/tar.gz/2]  skills".to_string(),
            ),
        ],
    );
}

#[test]
fn review_skills_with_no_pending_skills_returns_empty_review() {
    let approved = std::collections::BTreeSet::from(["esbuild".to_string()]);
    let pending = pending(&[("esbuild", &[Build])]);
    let review =
        super::review_skills(decide_config(), Path::new("/workspace"), &approved, &pending)
            .unwrap()
            .unwrap();
    assert!(review.skills_by_package.is_empty());
    assert!(review.dirs.is_empty());
}

#[test]
fn clear_decided_removes_decided_entries_from_modules_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    fs::create_dir_all(&modules_dir).unwrap();
    let modules = pnpm_modules_yaml::Modules {
        ignored_builds: Some(indexmap::IndexSet::from([pnpm_modules_yaml::DepPath::from(
            "foo@1.0.0".to_string(),
        )])),
        pending_skills: Some(indexmap::IndexSet::from([pnpm_modules_yaml::DepPath::from(
            "bar@1.0.0".to_string(),
        )])),
        ..Default::default()
    };
    pnpm_modules_yaml::write_modules_manifest::<pnpm_modules_yaml::Host>(&modules_dir, modules)
        .unwrap();

    let scan = crate::cli_args::ignored_builds::IgnoredBuildsScan {
        modules_dir: modules_dir.clone(),
        modules_manifest: pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(
            &modules_dir,
        )
        .unwrap(),
        names: None,
    };
    let mut decisions = BTreeMap::new();
    decisions.insert("foo".to_string(), decided(Some(true), None));
    decisions.insert("bar".to_string(), decided(None, Some(SkillsPermission::All(true))));

    super::decisions::clear_decided(scan, &decisions).unwrap();

    let updated = pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(&modules_dir)
        .unwrap()
        .unwrap();
    assert!(updated.ignored_builds.is_none());
    assert!(updated.pending_skills.is_none());
}
