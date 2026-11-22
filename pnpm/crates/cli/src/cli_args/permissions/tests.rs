use super::{
    ApproveArgs,
    Capabilities,
    PromptAvailability,
    decisions::{
        PackageDecisions,
        confirmation,
    },
    render_permissions,
};
use crate::cli_args::approve_builds::ApproveBuildsError;
use pnpm_config::{
    Config,
    PermissionCapability::{
        Build,
        Skills,
    },
    SkillsPermission,
};
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
};
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
        global: false,
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
fn an_explicit_git_repository_selector_decides_pending_commits() {
    let repo = "example@git+https://example.org/project.git";
    let first = format!("{repo}#1111111");
    let second = format!("{repo}#2222222");
    let other = "example@git+https://example.org/other.git#1111111";
    let pending = pending(&[(&first, &[Build]), (&second, &[Build]), (other, &[Build, Skills])]);
    let approval = args(&[repo], false)
        .decide::<SilentReporter>(&pending, decide_config(), Path::new("/workspace"))
        .unwrap()
        .unwrap();
    assert_eq!(approval.decisions, BTreeMap::from([(repo.to_string(), decided(Some(true), None))]));
    assert_eq!(
        super::decisions::pending_builds_approved(&approval.decisions, &pending),
        vec![first, second],
    );
}

#[test]
fn approving_a_registry_package_keeps_the_same_name_git_request_pending() {
    let dir = tempfile::tempdir().unwrap();
    let git = "example@git+https://example.org/project.git#1111111";
    let config = config_awaiting(&dir, &["example@1.0.0", git]);
    let requests = super::pending_permissions(&config).unwrap();
    let approval = args(&["example"], false)
        .decide::<SilentReporter>(&requests.by_package, decide_config(), dir.path())
        .unwrap()
        .unwrap();
    assert_eq!(
        approval.decisions,
        BTreeMap::from([("example".into(), decided(None, Some(SkillsPermission::All(true))))]),
    );
    super::decisions::clear_decided(requests.scan, &approval.decisions).unwrap();
    assert_eq!(
        super::pending_permissions(&config).unwrap().by_package,
        pending(&[(git, &[Skills])]),
    );
}

#[test]
fn an_unmatched_git_selector_lists_the_repository_and_each_pending_commit_once() {
    let repo = "sample@git+https://example.org/project.git";
    let first = format!("{repo}#1111111");
    let second = format!("{repo}#2222222");
    let requests = pending(&[(&first, &[Build]), (&second, &[Skills])]);
    for invalid in ["sample", &format!("{repo}#3333333")] {
        let error = args(&[invalid], false)
            .decide::<SilentReporter>(&requests, decide_config(), Path::new("/workspace"))
            .unwrap_err();
        let Some(ApproveBuildsError::ExactSelectorRequired(selectors)) = error.downcast_ref()
        else {
            panic!("unexpected error: {error}");
        };
        assert_eq!(selectors.as_slice(), [repo, first.as_str(), second.as_str()]);
    }
}

#[test]
fn a_git_repository_decision_clears_only_its_pending_skills() {
    let dir = tempfile::tempdir().unwrap();
    let other = "example@git+https://example.org/other.git#1111111";
    let repo = "example@git+https://example.org/project.git";
    let config = config_awaiting(&dir, &[&format!("{repo}#1111111"), other]);
    let pending = super::pending_permissions(&config).unwrap();
    let approval = args(&[repo], false)
        .decide::<SilentReporter>(&pending.by_package, decide_config(), dir.path())
        .unwrap()
        .unwrap();
    assert_eq!(
        approval.decisions,
        BTreeMap::from([(repo.to_string(), decided(None, Some(SkillsPermission::All(true))))]),
    );
    super::decisions::clear_decided(pending.scan, &approval.decisions).unwrap();
    assert_eq!(
        super::pending_permissions(&config).unwrap().by_package,
        self::pending(&[(other, &[Skills])]),
    );
}

#[test]
fn repository_scope_combines_all_pending_commits_before_review() {
    let repo = "demo@git+https://example.org/demo.git";
    let first = format!("{repo}#abcdef1");
    let second = format!("{repo}#abcdef2");
    let other = "demo@git+https://example.org/other.git#abcdef1";
    let requests = pending(&[(&first, &[Build]), (&second, &[Skills]), (other, &[Skills])]);
    let selected = [repo.to_string()].into();
    let grouped = super::repository_scope::group_requests(requests, &selected);
    assert_eq!(grouped, pending(&[(repo, &[Build, Skills]), (other, &[Skills])]));
    let skills = BTreeMap::from([(first, vec!["one"]), (second, vec!["two", "one"])]);
    assert_eq!(
        super::repository_scope::group_requests(skills, &selected),
        BTreeMap::from([(repo.to_string(), ["one", "two"].into())]),
    );
}

#[test]
fn approving_all_git_packages_keeps_the_resolved_commit() {
    let commit = "example@git+https://example.org/project.git#1111111";
    let approval = args(&[], true)
        .decide::<SilentReporter>(
            &pending(&[(commit, &[Skills])]),
            decide_config(),
            Path::new("/workspace"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        approval.decisions,
        BTreeMap::from([(commit.to_string(), decided(None, Some(SkillsPermission::All(true))))]),
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
fn rejects_interactive_approval_when_prompting_is_unavailable() {
    let pending = pending(&[("esbuild", &[Build])]);
    let error = args(&[], false)
        .decide_with_prompt::<SilentReporter>(
            &pending,
            decide_config(),
            Path::new("/workspace"),
            PromptAvailability::Unavailable,
        )
        .expect_err("interactive approval without a prompt is rejected");
    assert_eq!(
        error
            .code()
            .map(|code| code.to_string())
            .as_deref(),
        Some("ERR_PNPM_APPROVE_NON_INTERACTIVE"),
    );

    assert!(
        args(&[], false)
            .decide_with_prompt::<SilentReporter>(
                &BTreeMap::new(),
                decide_config(),
                Path::new("/workspace"),
                PromptAvailability::Unavailable,
            )
            .expect("no pending permissions needs no prompt")
            .is_none(),
    );
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
    let outcome = super::skills_review::prompt_for_package_skills("foo", &[]).unwrap();
    assert_eq!(outcome, super::skills_review::SkillPromptOutcome::Undiscovered);
}

#[test]
fn a_single_skill_is_approved_with_its_package_without_a_prompt() {
    let outcome =
        super::skills_review::prompt_for_package_skills("foo", &["guide".to_string()]).unwrap();
    assert_eq!(
        outcome,
        super::skills_review::SkillPromptOutcome::Decided(SkillsPermission::All(true))
    );
}

#[test]
fn review_skills_with_undiscovered_skills_leaves_package_out_of_review() {
    let approved = std::collections::BTreeSet::from(["foo".to_string()]);
    let pending = pending(&[("foo", &[Skills])]);
    let review = super::skills_review::review_skills(
        decide_config(),
        Path::new("/workspace"),
        &approved,
        &pending,
    )
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
    let review = super::skills_review::review_skills(
        decide_config(),
        Path::new("/workspace"),
        &approved,
        &pending,
    )
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

#[test]
fn pending_permissions_and_clear_decided_key_local_directory_skills_by_package_name() {
    let dir = tempfile::tempdir().unwrap();
    let config = config_awaiting(
        &dir,
        &[
            "@example/my-skills@link:packages/my-skills",
            "@example/file-skills@file:../file-skills",
            "tar-skills@file:./tar-skills.tgz",
        ],
    );

    let actual = super::pending_permissions(&config).unwrap();
    assert_eq!(
        actual.by_package,
        pending(&[
            ("@example/file-skills", &[Skills]),
            ("@example/my-skills", &[Skills]),
            ("tar-skills@file:./tar-skills.tgz", &[Skills]),
        ]),
    );

    let mut decisions = BTreeMap::new();
    decisions.insert(
        "@example/my-skills".to_string(),
        decided(None, Some(SkillsPermission::All(true))),
    );
    decisions.insert(
        "@example/file-skills".to_string(),
        decided(None, Some(SkillsPermission::All(false))),
    );
    super::decisions::clear_decided(actual.scan, &decisions).unwrap();

    let updated = pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(
        &dir.path().join("node_modules"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        updated.pending_skills,
        Some(indexmap::IndexSet::from([pnpm_modules_yaml::DepPath::from(
            "tar-skills@file:./tar-skills.tgz".to_string(),
        )])),
    );
}

/// A run that names no packages leaves a recorded build denial alone, even
/// when the stale `.modules.yaml` still lists the build as ignored.
#[test]
fn a_denied_build_is_not_awaiting_approval_for_an_unnamed_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config_awaiting(&dir, &["drizzle-kit@1.0.0"]);
    config.allow_builds.insert("drizzle-kit".to_string(), false);
    config.allow_builds.insert("esbuild".to_string(), false);
    let mut pending = super::PendingPermissions {
        by_package: pending(&[("drizzle-kit", &[Build, Skills]), ("esbuild", &[Build])]),
        scan: crate::cli_args::ignored_builds::get_automatically_ignored_builds(&config).unwrap(),
    };

    pending.exclude_denied_builds(&config).unwrap();

    assert_eq!(pending.by_package, self::pending(&[("drizzle-kit", &[Skills])]));
}
