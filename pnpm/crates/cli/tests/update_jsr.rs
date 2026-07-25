//! `pacquet update --latest` over `jsr:` dependencies, ported from
//! `pnpm11/installing/commands/test/update/jsr.ts`.
//!
//! Upstream pins `latest` with `addDistTag` first; the fixture registry
//! already serves 2.0.0 as the highest published version of
//! `@jsr/pnpm-e2e__bar`, so the ports assert the same end state without
//! it.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, PkgName};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

/// `jsr:` specifiers resolve through the `@jsr` scope, which defaults to
/// `npm.jsr.io`; point it at the mocked registry, which serves the
/// `@jsr/pnpm-e2e__bar` fixture. Upstream does the same through its
/// `registries: { '@jsr': ... }` option.
fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let npmrc = fs::read_to_string(&npmrc_info.npmrc_path).expect("read the harness .npmrc");
    let jsr_registry = npmrc_info.mock_instance.url();
    fs::write(&npmrc_info.npmrc_path, format!("{npmrc}@jsr:registry={jsr_registry}\n"))
        .expect("write .npmrc");
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-update-jsr", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

fn dep_spec(workspace: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

/// The lockfile's recorded `(specifier, version)` for a root dependency.
fn lockfile_entry(workspace: &Path, alias: &str) -> Option<(String, String)> {
    let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&text)
        .unwrap_or_else(|error| panic!("parse pnpm-lock.yaml: {error}\n{text}"));
    let alias: PkgName = alias.parse().expect("parse alias");
    let entry =
        lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY)?.dependencies.as_ref()?.get(&alias)?;
    Some((entry.specifier.clone(), entry.version.to_string()))
}

/// The half of `jsr without alias` pacquet already satisfies: the plain
/// form resolves and is recorded as a direct dependency.
#[test]
fn install_records_a_plain_jsr_dependency() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, r#"{ "@pnpm-e2e/bar": "jsr:1.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    assert_eq!(
        lockfile_entry(&workspace, "@pnpm-e2e/bar"),
        Some(("jsr:1.0.0".to_string(), "@jsr/pnpm-e2e__bar@1.0.0".to_string())),
    );

    drop((root, anchor));
}

mod known_failures {
    use super::{dep_spec, lockfile_entry, pacquet, setup, write_manifest};
    use assert_cmd::prelude::*;
    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };
    use pretty_assertions::assert_eq;

    /// `parse_jsr_specifier_to_registry_package_spec` builds a spec with
    /// no normalized bare specifier, so `update --latest` has nothing to
    /// write back and the `jsr:` version stays put. See pnpm/pnpm#13363.
    fn jsr_specifiers_are_not_rewritten_by_latest() -> KnownResult<()> {
        Err(KnownFailure::new(
            "pacquet's jsr specifier parser produces no normalized bare specifier, so \
             `update --latest` leaves the `jsr:` version untouched.",
        ))
    }

    /// An aliased `jsr:` dependency resolves into `packages` and
    /// `snapshots` but leaves the importer empty, so the lockfile never
    /// records it as a direct dependency. See pnpm/pnpm#13362.
    fn an_aliased_jsr_dependency_is_missing_from_the_importer() -> KnownResult<()> {
        Err(KnownFailure::new(
            "pacquet resolves an aliased `jsr:` dependency but leaves the importer empty, \
             so the lockfile does not record it as a direct dependency.",
        ))
    }

    /// Ports `jsr without alias`: the manifest keeps the `jsr:` protocol
    /// and the version behind it is bumped, while the lockfile records
    /// the resolved `@jsr/` package.
    #[test]
    fn update_latest_bumps_a_jsr_dependency() {
        let (root, workspace, anchor) = setup();

        write_manifest(&workspace, r#"{ "@pnpm-e2e/bar": "jsr:1.0.0" }"#);
        pacquet(&workspace, ["install"]).assert().success();
        pacquet(&workspace, ["update", "--latest"]).assert().success();
        allow_known_failure!(jsr_specifiers_are_not_rewritten_by_latest());

        assert_eq!(dep_spec(&workspace, "@pnpm-e2e/bar").as_deref(), Some("jsr:2.0.0"));
        assert_eq!(
            lockfile_entry(&workspace, "@pnpm-e2e/bar"),
            Some(("jsr:2.0.0".to_string(), "@jsr/pnpm-e2e__bar@2.0.0".to_string())),
        );

        drop((root, anchor));
    }

    /// Ports `jsr with alias`: the aliased form keeps both the alias and
    /// the package name it resolves through, bumping only the version.
    #[test]
    fn update_latest_bumps_an_aliased_jsr_dependency() {
        let (root, workspace, anchor) = setup();

        write_manifest(&workspace, r#"{ "bar-from-jsr": "jsr:@pnpm-e2e/bar@1.0.0" }"#);
        pacquet(&workspace, ["install"]).assert().success();
        allow_known_failure!(an_aliased_jsr_dependency_is_missing_from_the_importer());

        assert_eq!(
            lockfile_entry(&workspace, "bar-from-jsr"),
            Some(("jsr:@pnpm-e2e/bar@1.0.0".to_string(), "@jsr/pnpm-e2e__bar@1.0.0".to_string())),
        );

        pacquet(&workspace, ["update", "--latest"]).assert().success();
        allow_known_failure!(jsr_specifiers_are_not_rewritten_by_latest());

        assert_eq!(
            dep_spec(&workspace, "bar-from-jsr").as_deref(),
            Some("jsr:@pnpm-e2e/bar@2.0.0"),
        );
        assert_eq!(
            lockfile_entry(&workspace, "bar-from-jsr"),
            Some(("jsr:@pnpm-e2e/bar@2.0.0".to_string(), "@jsr/pnpm-e2e__bar@2.0.0".to_string())),
        );

        drop((root, anchor));
    }
}
