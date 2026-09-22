use super::{WantedLockfileSatisfactionCheck, wanted_lockfile_satisfies_workspace};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn workspace_satisfaction_uses_pinned_lockfile_dir_for_local_overrides() {
    let root = tempdir().expect("create fixture directory");
    let workspace_dir = root.path().join("workspace");
    let project_dir = workspace_dir.join("pkgs/a");
    let lockfile_dir = root.path().join("locks");
    fs::create_dir_all(&project_dir).expect("create nested project");
    fs::create_dir_all(&lockfile_dir).expect("create lockfile directory");
    fs::write(workspace_dir.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root manifest");
    fs::write(workspace_dir.join("pnpm-workspace.yaml"), "packages:\n  - pkgs/*\n")
        .expect("write workspace manifest");
    fs::write(project_dir.join("package.json"), r#"{"name":"a","dependencies":{"foo":"*"}}"#)
        .expect("write nested manifest");
    let manifest =
        PackageManifest::from_path(project_dir.join("package.json")).expect("read nested manifest");
    let mut config = Config::new();
    config.workspace_dir = Some(workspace_dir);
    config.lockfile_dir = Some(lockfile_dir);
    config.ignore_pnpmfile = true;
    config.overrides =
        Some(indexmap::IndexMap::from([("foo".to_string(), "link:./foo".to_string())]));
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"lockfileVersion: '9.0'
overrides:
  foo: link:./foo
importers:
  ../workspace: {}
  ../workspace/pkgs/a:
    dependencies:
      foo:
        specifier: link:../../../locks/foo
        version: link:../../../locks/foo
",
    )
    .expect("parse lockfile");
    let catalogs = Catalogs::new();
    let check = WantedLockfileSatisfactionCheck {
        config: &config,
        manifest: &manifest,
        catalogs: &catalogs,
        lockfile: &lockfile,
        ignore_manifest_check: false,
    };
    assert!(
        wanted_lockfile_satisfies_workspace(&check).await,
        "a nested project's override must resolve from the pinned lockfile directory",
    );

    let mut stale_lockfile = lockfile.clone();
    stale_lockfile.importers
        .get_mut("../workspace/pkgs/a")
        .expect("nested importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .values_mut()
        .next()
        .expect("foo dependency")
        .specifier = "link:foo".to_string();
    assert!(
        !wanted_lockfile_satisfies_workspace(&WantedLockfileSatisfactionCheck {
            lockfile: &stale_lockfile,
            ..check
        })
        .await,
        "an override incorrectly anchored at the nested project must be rejected",
    );
}

/// An `optionalDependencies` entry the lockfile has no importer entry for
/// was skipped by the install that wrote it. Only the frozen path may treat
/// it as satisfied, and it must not excuse a missing regular dependency
/// ([pnpm/pnpm#3960](https://github.com/pnpm/pnpm/issues/3960)).
#[test]
fn unresolved_optional_dependency_is_satisfied_only_when_allowed() {
    let root = tempdir().expect("create fixture directory");
    fs::write(
        root.path().join("package.json"),
        r#"{"name":"root","dependencies":{"is-negative":"1.0.0"},"optionalDependencies":{"is-positive":"^30000.0.0"}}"#,
    )
    .expect("write manifest");
    let manifest =
        PackageManifest::from_path(root.path().join("package.json")).expect("read manifest");
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-negative:
        specifier: 1.0.0
        version: 1.0.0
",
    )
    .expect("parse lockfile");
    let config = Config::new();
    let ignored_optional_matcher = pnpm_matcher::create_matcher(&[]);
    fn importer_check<'a>(
        (lockfile, lockfile_dir, config): (&'a Lockfile, &'a std::path::Path, &'a Config),
        manifest: &'a PackageManifest,
        (ignored, allow_unresolved): (&'a pnpm_matcher::Matcher, bool),
    ) -> super::ImporterSatisfactionCheck<'a> {
        super::ImporterSatisfactionCheck {
            lockfile,
            lockfile_dir,
            manifest,
            importer_id: ".",
            config,
            workspace_packages: None,
            optional_exclusions: super::OptionalDependencyExclusions { ignored, allow_unresolved },
            parsed_overrides: None,
        }
    }
    let fixture = (&lockfile, root.path(), &config);
    let check = |manifest: &PackageManifest, allow_unresolved: bool| {
        super::check_importer_satisfies(&importer_check(
            fixture,
            manifest,
            (&ignored_optional_matcher, allow_unresolved),
        ))
    };

    let error = check(&manifest, false).expect_err("a resolving install retries the optional");
    assert!(
        matches!(
            &error,
            super::FreshnessCheckError::Stale(pnpm_lockfile::StalenessReason::SpecifiersDiffer(
                diff
            )) if diff.added.contains_key("is-positive")
        ),
        "expected is-positive reported as added, got {error:?}",
    );
    check(&manifest, true).expect("a frozen install skips the optional again");

    let importer = lockfile.importers.get(".").expect("root importer");
    let skipped = super::unresolved_optional_dependencies(
        &importer_check(fixture, &manifest, (&ignored_optional_matcher, true)),
        importer,
    );
    assert_eq!(skipped, vec![("is-positive".to_string(), "^30000.0.0".to_string())]);

    fs::write(
        root.path().join("package.json"),
        r#"{"name":"root","dependencies":{"is-negative":"1.0.0","is-odd":"1.0.0"},"optionalDependencies":{"is-positive":"^30000.0.0"}}"#,
    )
    .expect("write manifest with an added dependency");
    let manifest =
        PackageManifest::from_path(root.path().join("package.json")).expect("read manifest");
    let error = check(&manifest, true).expect_err("an added regular dependency is still drift");
    assert!(
        matches!(
            &error,
            super::FreshnessCheckError::Stale(pnpm_lockfile::StalenessReason::SpecifiersDiffer(
                diff
            )) if diff.added.contains_key("is-odd") && !diff.added.contains_key("is-positive")
        ),
        "expected only is-odd reported as added, got {error:?}",
    );
}
