//! `pnpm audit <pkg>...`: audit packages by name, without a project.

use super::{
    AuditArgs, AuditError, AuditOutcome, Config, DependencyGroup, IntoDiagnostic, Path,
    RangeSpecStyle, State, correct_inferred_patched_versions,
};
use crate::cli_args::{
    add::{AddGroups, add_packages},
    dlx::configure_cache_install,
    supported_architectures::SupportedArchitecturesArgs,
};
use miette::Context;
use pnpm_reporter::SilentReporter;
use std::path::PathBuf;
use tempfile::TempDir;

impl AuditArgs {
    /// Whether the command line names packages to audit rather than
    /// auditing the project or running the `signatures` subcommand.
    pub(crate) fn names_packages(&self) -> bool {
        self.params
            .first()
            .is_some_and(|first| first != "signatures")
    }

    /// Audit the packages the command line names, with no project: resolve
    /// them, lockfile only, into a throwaway project and audit its lockfile.
    pub async fn run_packages(self, config: &'static mut Config) -> miette::Result<AuditOutcome> {
        if self.fix.is_some()
            || self.interactive
            || !self.advisories.ignore.is_empty()
            || self.advisories.ignore_unfixable
        {
            return Err(AuditError::ProjectOptionWithPackages.into());
        }
        let (temp_dir, manifest_path) = create_throwaway_project()?;
        configure_cache_install(
            config,
            temp_dir.path(),
            &[],
            &[],
            &SupportedArchitecturesArgs::default(),
        )?;
        let config: &'static Config = config;
        resolve_packages(&manifest_path, &self.params, config).await?;
        let state =
            State::init(manifest_path, config, true).wrap_err("initialize the audit state")?;
        self.audit_resolved(&state, temp_dir.path()).await
    }

    async fn audit_resolved(
        &self,
        state: &State,
        project_dir: &Path,
    ) -> miette::Result<AuditOutcome> {
        let config = state.config;
        let lockfile = state.lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?
            .ok_or(AuditError::NoLockfile)?;
        let include = self.dependency_options.include(config);
        let audit_level = self.advisories.effective_level(config.audit_level);
        let Some(mut report) =
            self.audit_lockfile(state, lockfile, include, audit_level, project_dir).await?
        else {
            return Ok(AuditOutcome::Clean);
        };
        correct_inferred_patched_versions(&mut report, config, state.http_client.as_ref()).await;
        self.render_report(report, config, project_dir, audit_level)
    }
}

/// A temporary directory holding an empty `package.json`, removed on drop.
fn create_throwaway_project() -> miette::Result<(TempDir, PathBuf)> {
    let temp_dir = tempfile::Builder::new()
        .prefix("pnpm-audit-")
        .tempdir()
        .into_diagnostic()
        .wrap_err("create a directory to resolve the audited packages in")?;
    let manifest_path = temp_dir.path().join("package.json");
    std::fs::write(&manifest_path, r#"{"name":"pnpm-audit","version":"0.0.0","private":true}"#)
        .into_diagnostic()
        .wrap_err("write the audit project's package.json")?;
    Ok((temp_dir, manifest_path))
}

/// Add `specs` to the project at `manifest_path`, writing only its lockfile.
async fn resolve_packages(
    manifest_path: &Path,
    specs: &[String],
    config: &'static Config,
) -> miette::Result<()> {
    let state = State::init(manifest_path.to_path_buf(), config, false)
        .wrap_err("initialize the audit resolution state")?;
    add_packages::<SilentReporter, _>(
        state,
        specs,
        // Pinned, so the lockfile holds the version each spec resolved to
        // rather than the newest one a saved caret range admits.
        RangeSpecStyle::Patch,
        None,
        true,
        config.supported_architectures.clone(),
        AddGroups { save_target: Some([DependencyGroup::Prod]), included: None, save_types: false },
    )
    .await
}
