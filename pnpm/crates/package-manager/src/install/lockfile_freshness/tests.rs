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
    stale_lockfile
        .importers
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
