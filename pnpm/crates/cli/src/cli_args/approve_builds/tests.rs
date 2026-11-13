use super::{ApproveBuildsArgs, ApproveBuildsError, partition_params, sort_unique};
use pnpm_reporter::SilentReporter;

fn pending(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

fn params(args: &[&str]) -> Vec<String> {
    args.iter()
        .map(|arg| (*arg).to_string())
        .collect()
}

fn args(packages: &[&str]) -> ApproveBuildsArgs {
    ApproveBuildsArgs { packages: params(packages), all: false, global: false }
}

fn approve_builds_error(report: miette::Report) -> ApproveBuildsError {
    report.downcast::<ApproveBuildsError>().expect("an approve-builds error")
}

#[test]
fn splits_approved_and_denied() {
    let partition = partition_params(&params(&["foo", "!bar"]), &pending(&["foo", "bar"]));
    assert_eq!(partition.approved, vec!["foo".to_string()]);
    assert_eq!(partition.denied, vec!["bar".to_string()]);
    assert!(partition.unknown.is_empty());
}

#[test]
fn reports_an_unknown_approved_package_as_pre_emptive() {
    let partition = partition_params(&params(&["nope"]), &pending(&["foo"]));
    assert_eq!(partition.approved, vec!["nope".to_string()]);
    assert!(partition.denied.is_empty());
    assert_eq!(partition.unknown, vec!["nope".to_string()]);
}

#[test]
fn reports_an_unknown_denied_package_as_pre_emptive() {
    let partition = partition_params(&params(&["!nope"]), &pending(&["foo"]));
    assert!(partition.approved.is_empty());
    assert_eq!(partition.denied, vec!["nope".to_string()]);
    assert_eq!(partition.unknown, vec!["nope".to_string()]);
}

// Ports pnpm's `contradictory arguments throw error`.
#[test]
fn rejects_contradictory_arguments() {
    let err = args(&["foo", "!foo"])
        .decide::<SilentReporter>(&pending(&["foo"]))
        .err()
        .expect("contradicting arguments are rejected");
    let ApproveBuildsError::ContradictingArgs(names) = approve_builds_error(err) else {
        panic!("expected ContradictingArgs");
    };
    assert_eq!(names, vec!["foo".to_string()]);
}

#[test]
fn rejects_an_argument_that_names_no_package() {
    for packages in [&["!"][..], &[""][..], &["foo", "!"][..]] {
        let err = args(packages).validate().unwrap_err();
        assert!(
            matches!(approve_builds_error(err), ApproveBuildsError::MissingPackage),
            "expected MissingPackage for {packages:?}",
        );
    }
}

#[test]
fn rejects_positional_arguments_with_all() {
    let err = ApproveBuildsArgs { packages: params(&["foo"]), all: true, global: false }
        .validate()
        .unwrap_err();
    assert!(matches!(approve_builds_error(err), ApproveBuildsError::AllWithArgs));
}

#[test]
fn sort_unique_dedupes_and_sorts() {
    assert_eq!(sort_unique(params(&["b", "a", "b"])), vec!["a".to_string(), "b".to_string()]);
}

/// The approval run records `skills.dirs` into the manifest after its
/// config was loaded, so the overlay must carry the freshly written
/// value into the linking step.
#[test]
fn overlays_the_manifests_skills_dirs() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "permissions:\n  '@pnpm.e2e/with-agent-skills':\n    skills: true\nskills:\n  dirs:\n    - .claude/skills\n",
    )
    .expect("write pnpm-workspace.yaml");

    let approved =
        super::config_with_install_approvals(&pnpm_config::Config::default(), dir.path())
            .expect("lay the approvals over the config");

    assert_eq!(approved.skills_dirs.as_deref(), Some([".claude/skills".to_string()].as_slice()));
    assert_eq!(
        approved.allow_skills.get("@pnpm.e2e/with-agent-skills").cloned(),
        Some(pnpm_config::SkillsPermission::All(true)),
    );
}

#[test]
fn confirm_builds_prompt_formats_short_labels() {
    assert_eq!(
        super::confirm_builds_prompt(
            &["esbuild@0.25.0".to_string(), "sharp".to_string()],
            &["esbuild@0.25.0".to_string(), "sharp".to_string()],
        ),
        "Build esbuild, sharp?",
    );
}

#[test]
fn confirm_builds_prompt_keeps_colliding_labels_distinguishable() {
    let pending = [
        "foo@https://codeload.github.com/org1/foo/tar.gz/1".to_string(),
        "foo@https://codeload.github.com/org2/foo/tar.gz/2".to_string(),
    ];
    assert_eq!(
        super::confirm_builds_prompt(&pending[..1], &pending),
        "Build foo (git) [foo@https://codeload.github.com/org1/foo/tar.gz/1]?",
    );
}

#[test]
fn build_choices_distinguishes_colliding_labels() {
    let keys = vec![
        "foo@https://codeload.github.com/org1/foo/tar.gz/1".to_string(),
        "foo@https://codeload.github.com/org2/foo/tar.gz/2".to_string(),
        "bar@1.0.0".to_string(),
    ];
    let choices = super::prompt::build_choices(&keys);
    assert_eq!(
        choices,
        vec![
            (
                "foo@https://codeload.github.com/org1/foo/tar.gz/1".to_string(),
                "foo (git) [foo@https://codeload.github.com/org1/foo/tar.gz/1]".to_string(),
            ),
            (
                "foo@https://codeload.github.com/org2/foo/tar.gz/2".to_string(),
                "foo (git) [foo@https://codeload.github.com/org2/foo/tar.gz/2]".to_string(),
            ),
            ("bar@1.0.0".to_string(), "bar".to_string()),
        ],
    );
}

#[test]
fn prepare_with_filtered_scan_excludes_explicitly_denied_builds() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let manifest_path = dir.path().join("package.json");
    std::fs::write(&manifest_path, r#"{"name":"app","version":"1.0.0"}"#).expect("write manifest");

    let mut config = pnpm_config::Config {
        workspace_dir: Some(dir.path().to_path_buf()),
        ..pnpm_config::Config::default()
    };
    config.allow_builds.insert("denied-pkg".to_string(), false);

    let mut scan = crate::cli_args::ignored_builds::IgnoredBuildsScan {
        names: Some(vec!["denied-pkg".to_string(), "pending-pkg".to_string()]),
        modules_dir: dir.path().join("node_modules"),
        modules_manifest: None,
    };
    super::exclude_disallowed_builds(&mut scan, &config);
    assert_eq!(scan.names.as_deref(), Some(["pending-pkg".to_string()].as_slice()));

    let (_, build_packages) = ApproveBuildsArgs { packages: Vec::new(), all: true, global: false }
        .prepare_with_scan::<SilentReporter>(dir.path(), &config, &config, &manifest_path, scan)
        .expect("prepare install approvals")
        .expect("pending-pkg awaits rebuild");

    assert_eq!(build_packages, vec!["pending-pkg".to_string()]);
    let overlaid =
        super::config_with_install_approvals(&config, dir.path()).expect("overlay approvals");
    assert_eq!(overlaid.allow_builds.get("denied-pkg"), Some(&false));
    assert_eq!(overlaid.allow_builds.get("pending-pkg"), Some(&true));
}
