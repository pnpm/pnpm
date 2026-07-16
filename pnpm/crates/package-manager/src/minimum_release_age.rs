use std::{marker::PhantomData, path::Path};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::{
    Config,
    version_policy::{VersionPolicyError, merge_package_version_specs},
};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, PromptAction, PromptLog, Reporter};
use pacquet_resolving_resolver_base::ResolutionPolicyViolation;
use pacquet_workspace_manifest_writer::{
    UpdateWorkspaceManifestError, set_minimum_release_age_excludes,
};

use pacquet_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;

#[derive(Debug, Display, Error, Diagnostic)]
pub enum MinimumReleaseAgeError {
    #[display(
        "minimumReleaseAgeStrict cannot be combined with --no-save: approval would require writing to minimumReleaseAgeExclude in pnpm-workspace.yaml, which --no-save prevents."
    )]
    #[diagnostic(
        code(ERR_PNPM_STRICT_MIN_RELEASE_AGE_REQUIRES_SAVE),
        help(
            "Drop --no-save so the exclude list can be persisted, or set minimumReleaseAgeStrict: false."
        )
    )]
    StrictRequiresSave,

    #[display("{message}")]
    #[diagnostic(
        code(ERR_PNPM_NO_MATURE_MATCHING_VERSION),
        help(
            "Run the install interactively to approve these picks, add them to minimumReleaseAgeExclude in pnpm-workspace.yaml, or wait for the packages to mature."
        )
    )]
    NoMatureMatchingVersion { message: String },

    #[display("Aborted: the immature versions were not approved.")]
    #[diagnostic(
        code(ERR_PNPM_MINIMUM_RELEASE_AGE_DENIED),
        help(
            "Re-run without minimumReleaseAgeStrict: true, or wait for the packages to mature past the configured cutoff."
        )
    )]
    Denied,

    #[display("Failed to read minimumReleaseAge approval: {_0}")]
    #[diagnostic(code(pacquet_package_manager::minimum_release_age_prompt))]
    Prompt(#[error(source)] dialoguer::Error),

    #[diagnostic(transparent)]
    VersionPolicy(#[error(source)] VersionPolicyError),

    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),
}

pub(crate) fn ensure_strict_minimum_release_age_can_save(
    config: &Config,
    save: bool,
) -> Result<(), MinimumReleaseAgeError> {
    if !save
        && config.resolved_minimum_release_age().is_some()
        && config.resolved_minimum_release_age_strict()
    {
        return Err(MinimumReleaseAgeError::StrictRequiresSave);
    }
    Ok(())
}

pub(crate) async fn handle_minimum_release_age_violations<ReporterImpl: Reporter>(
    config: &Config,
    workspace_dir: &Path,
    violations: &[ResolutionPolicyViolation],
    can_prompt: bool,
) -> Result<(), MinimumReleaseAgeError> {
    handle_minimum_release_age_violations_with::<ReporterImpl, _>(
        config,
        workspace_dir,
        violations,
        can_prompt,
        &mut DialoguerPrompt,
    )
    .await
}

trait ApprovalPrompt {
    async fn confirm(&mut self, message: &str) -> dialoguer::Result<bool>;
}

struct DialoguerPrompt;

impl ApprovalPrompt for DialoguerPrompt {
    async fn confirm(&mut self, message: &str) -> dialoguer::Result<bool> {
        let message = message.to_owned();
        tokio::task::spawn_blocking(move || {
            dialoguer::Confirm::new().with_prompt(message).default(false).interact()
        })
        .await
        .map_err(|error| dialoguer::Error::IO(std::io::Error::other(error)))?
    }
}

async fn handle_minimum_release_age_violations_with<ReporterImpl, Prompt>(
    config: &Config,
    workspace_dir: &Path,
    violations: &[ResolutionPolicyViolation],
    can_prompt: bool,
    prompt: &mut Prompt,
) -> Result<(), MinimumReleaseAgeError>
where
    ReporterImpl: Reporter,
    Prompt: ApprovalPrompt,
{
    if !config.resolved_minimum_release_age_strict() {
        return Ok(());
    }

    let immature = sorted_immature_violations(violations);
    if immature.is_empty() {
        return Ok(());
    }
    if !can_prompt {
        return Err(MinimumReleaseAgeError::NoMatureMatchingVersion {
            message: format_violation_error(&immature),
        });
    }

    let message = format_prompt(&immature);
    let confirmed = {
        let _guard = PromptGuard::<ReporterImpl>::new();
        prompt.confirm(&message).await.map_err(MinimumReleaseAgeError::Prompt)?
    };
    if !confirmed {
        return Err(MinimumReleaseAgeError::Denied);
    }

    let added: Vec<String> = immature
        .iter()
        .map(|violation| format!("{}@{}", violation.name, violation.version))
        .collect();
    let merged = merge_package_version_specs(
        config.minimum_release_age_exclude.iter().flatten().chain(&added),
    )
    .map_err(MinimumReleaseAgeError::VersionPolicy)?;
    let added =
        merge_package_version_specs(&added).map_err(MinimumReleaseAgeError::VersionPolicy)?;
    set_minimum_release_age_excludes(workspace_dir, &merged)
        .map_err(MinimumReleaseAgeError::WriteWorkspaceManifest)?;

    ReporterImpl::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: format!(
            "Added {} {} to minimumReleaseAgeExclude in pnpm-workspace.yaml (approved at the prompt):\n  {}",
            added.len(),
            if added.len() == 1 { "entry" } else { "entries" },
            added.join("\n  "),
        ),
        prefix: workspace_dir.to_string_lossy().into_owned(),
    }));
    Ok(())
}

fn sorted_immature_violations(
    violations: &[ResolutionPolicyViolation],
) -> Vec<&ResolutionPolicyViolation> {
    let mut immature: Vec<_> = violations
        .iter()
        .filter(|violation| violation.code == MINIMUM_RELEASE_AGE_VIOLATION_CODE)
        .collect();
    immature.sort_by_cached_key(|violation| format!("{}@{}", violation.name, violation.version));
    immature
}

fn format_violation_error(violations: &[&ResolutionPolicyViolation]) -> String {
    format!(
        "{} {} not meet the minimumReleaseAge constraint:\n{}",
        violations.len(),
        if violations.len() == 1 { "version does" } else { "versions do" },
        violations
            .iter()
            .map(|violation| format!(
                "  {}@{} {}",
                violation.name, violation.version, violation.reason
            ))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn format_prompt(violations: &[&ResolutionPolicyViolation]) -> String {
    format!(
        "{} {} not meet the minimumReleaseAge constraint:\n{}\nAdd to minimumReleaseAgeExclude in pnpm-workspace.yaml and proceed with the install?",
        violations.len(),
        if violations.len() == 1 { "version does" } else { "versions do" },
        violations
            .iter()
            .map(|violation| format!("  {}@{}", violation.name, violation.version))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

struct PromptGuard<ReporterImpl: Reporter>(PhantomData<ReporterImpl>);

impl<ReporterImpl: Reporter> PromptGuard<ReporterImpl> {
    fn new() -> Self {
        ReporterImpl::emit(&LogEvent::Prompt(PromptLog {
            level: LogLevel::Debug,
            action: PromptAction::Start,
        }));
        Self(PhantomData)
    }
}

impl<ReporterImpl: Reporter> Drop for PromptGuard<ReporterImpl> {
    fn drop(&mut self) {
        ReporterImpl::emit(&LogEvent::Prompt(PromptLog {
            level: LogLevel::Debug,
            action: PromptAction::End,
        }));
    }
}

#[cfg(test)]
mod tests;
