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
        super::confirm_builds_prompt(&["esbuild@0.25.0".to_string(), "sharp".to_string()]),
        "Build esbuild, sharp?",
    );
}
