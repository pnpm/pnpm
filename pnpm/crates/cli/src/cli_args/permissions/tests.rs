use super::{ApproveArgs, Capabilities, render_permissions};
use pnpm_config::{
    Config,
    PermissionCapability::{Build, Skills},
};
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

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

#[test]
fn a_named_package_is_decided_for_everything_it_requests() {
    let pending = pending(&[("drizzle-kit", &[Build, Skills]), ("esbuild", &[Build])]);

    let decisions = args(&["drizzle-kit", "!esbuild"], false)
        .decide::<SilentReporter>(&pending)
        .unwrap()
        .unwrap();

    assert_eq!(
        decisions,
        BTreeMap::from([
            (("drizzle-kit".to_string(), Build), true),
            (("drizzle-kit".to_string(), Skills), true),
            (("esbuild".to_string(), Build), false),
        ]),
    );
}

#[test]
fn a_package_named_before_it_is_pending_is_decided_for_every_capability() {
    let decisions = args(&["!foo"], false)
        .decide::<SilentReporter>(&BTreeMap::new())
        .unwrap()
        .unwrap();

    assert_eq!(
        decisions,
        BTreeMap::from([(("foo".to_string(), Build), false), (("foo".to_string(), Skills), false)]),
    );
}

#[test]
fn all_approves_every_pending_capability() {
    let pending = pending(&[("drizzle-kit", &[Skills]), ("esbuild", &[Build])]);

    let decisions = args(&[], true)
        .decide::<SilentReporter>(&pending)
        .unwrap()
        .unwrap();

    assert_eq!(
        decisions,
        BTreeMap::from([
            (("drizzle-kit".to_string(), Skills), true),
            (("esbuild".to_string(), Build), true),
        ]),
    );
}

#[test]
fn rejects_contradictory_arguments() {
    let error = args(&["foo", "!foo"], false)
        .decide::<SilentReporter>(&BTreeMap::new())
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
        allow_skills: [("drizzle-kit".to_string(), true)].into(),
        ..Config::default()
    };

    assert_eq!(
        render_permissions(&config).unwrap(),
        "Granted:\n  drizzle-kit  build, skills\n  esbuild      build\n\nDenied:\n  sharp  build\n",
    );
}
