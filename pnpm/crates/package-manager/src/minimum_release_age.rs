use std::{io::IsTerminal, marker::PhantomData, path::Path};

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

pub(crate) fn handle_minimum_release_age_violations<ReporterImpl: Reporter>(
    config: &Config,
    workspace_dir: &Path,
    violations: &[ResolutionPolicyViolation],
    allow_prompt: bool,
) -> Result<(), MinimumReleaseAgeError> {
    let can_prompt = allow_prompt && !is_ci::cached() && std::io::stdin().is_terminal();
    handle_minimum_release_age_violations_with::<ReporterImpl, _>(
        config,
        workspace_dir,
        violations,
        can_prompt,
        &mut DialoguerPrompt,
    )
}

trait ApprovalPrompt {
    fn confirm(&mut self, message: &str) -> dialoguer::Result<bool>;
}

struct DialoguerPrompt;

impl ApprovalPrompt for DialoguerPrompt {
    fn confirm(&mut self, message: &str) -> dialoguer::Result<bool> {
        dialoguer::Confirm::new().with_prompt(message).default(false).interact()
    }
}

fn handle_minimum_release_age_violations_with<ReporterImpl, Prompt>(
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
        prompt.confirm(&message).map_err(MinimumReleaseAgeError::Prompt)?
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
    set_minimum_release_age_excludes(workspace_dir, &merged)
        .map_err(MinimumReleaseAgeError::WriteWorkspaceManifest)?;

    let added =
        merge_package_version_specs(&added).map_err(MinimumReleaseAgeError::VersionPolicy)?;
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
mod tests {
    use std::{fs, sync::Mutex};

    use pacquet_config::Config;
    use pacquet_lockfile::{LockfileResolution, RegistryResolution};
    use pacquet_reporter::{LogEvent, PromptAction, Reporter};
    use pacquet_resolving_resolver_base::ResolutionPolicyViolation;
    use ssri::Integrity;
    use tempfile::tempdir;

    use super::{
        ApprovalPrompt, MinimumReleaseAgeError, ensure_strict_minimum_release_age_can_save,
        handle_minimum_release_age_violations_with,
    };

    fn violation(name: &str, version: &str, code: &'static str) -> ResolutionPolicyViolation {
        ResolutionPolicyViolation {
            name: name.parse().expect("valid package name"),
            version: version.to_string(),
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                    .parse::<Integrity>()
                    .expect("valid integrity"),
            }),
            code,
            reason: format!("{name}@{version} is too new"),
        }
    }

    #[derive(Default)]
    struct FakePrompt {
        answer: bool,
        messages: Vec<String>,
    }

    impl ApprovalPrompt for FakePrompt {
        fn confirm(&mut self, message: &str) -> dialoguer::Result<bool> {
            self.messages.push(message.to_string());
            Ok(self.answer)
        }
    }

    struct FailingPrompt;

    impl ApprovalPrompt for FailingPrompt {
        fn confirm(&mut self, _message: &str) -> dialoguer::Result<bool> {
            Err(std::io::Error::other("prompt input failed").into())
        }
    }

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct RecordingReporter;

    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("event lock").push(event.clone());
        }
    }

    #[test]
    fn strict_no_save_is_rejected_before_resolution() {
        let mut config = Config::new();
        config.minimum_release_age = Some(60);
        config.minimum_release_age_strict = Some(true);

        let error = ensure_strict_minimum_release_age_can_save(&config, false)
            .expect_err("strict mode requires persistence");

        assert!(matches!(error, MinimumReleaseAgeError::StrictRequiresSave));
        assert_eq!(
            error.to_string(),
            "minimumReleaseAgeStrict cannot be combined with --no-save: approval would require writing to minimumReleaseAgeExclude in pnpm-workspace.yaml, which --no-save prevents.",
        );
        assert!(ensure_strict_minimum_release_age_can_save(&config, true).is_ok());
        config.minimum_release_age = Some(0);
        assert!(ensure_strict_minimum_release_age_can_save(&config, false).is_ok());
    }

    #[test]
    fn non_interactive_strict_mode_reports_every_immature_pick() {
        let dir = tempdir().expect("temp dir");
        let mut config = Config::new();
        config.minimum_release_age_strict = Some(true);
        let mut prompt = FakePrompt::default();
        let violations = vec![
            violation("zeta", "2.0.0", "MINIMUM_RELEASE_AGE_VIOLATION"),
            violation("alpha", "1.0.0", "MINIMUM_RELEASE_AGE_VIOLATION"),
            violation("ignored", "3.0.0", "TRUST_DOWNGRADE"),
        ];

        let error = handle_minimum_release_age_violations_with::<RecordingReporter, _>(
            &config,
            dir.path(),
            &violations,
            false,
            &mut prompt,
        )
        .expect_err("non-interactive strict mode must fail");

        assert!(matches!(error, MinimumReleaseAgeError::NoMatureMatchingVersion { .. }));
        let message = error.to_string();
        assert!(message.starts_with("2 versions do not meet the minimumReleaseAge constraint:"));
        assert!(message.find("alpha@1.0.0").unwrap() < message.find("zeta@2.0.0").unwrap());
        assert!(!message.contains("ignored"));
        assert!(prompt.messages.is_empty());
    }

    #[test]
    fn approval_persists_canonical_excludes_and_brackets_the_prompt() {
        let _test_guard = TEST_LOCK.lock().expect("test lock");
        EVENTS.lock().expect("event lock").clear();
        let dir = tempdir().expect("temp dir");
        fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n",
        )
        .expect("write workspace manifest");
        let mut config = Config::new();
        config.minimum_release_age_strict = Some(true);
        config.minimum_release_age_exclude = Some(vec!["foo@1.0.0".to_string()]);
        let mut prompt = FakePrompt { answer: true, messages: Vec::new() };
        let violations = vec![
            violation("foo", "2.0.0", "MINIMUM_RELEASE_AGE_VIOLATION"),
            violation("bar", "3.0.0", "MINIMUM_RELEASE_AGE_VIOLATION"),
        ];

        handle_minimum_release_age_violations_with::<RecordingReporter, _>(
            &config,
            dir.path(),
            &violations,
            true,
            &mut prompt,
        )
        .expect("approval should continue");

        assert_eq!(prompt.messages.len(), 1);
        assert!(prompt.messages[0].contains("bar@3.0.0\n  foo@2.0.0"));
        let workspace = fs::read_to_string(dir.path().join("pnpm-workspace.yaml"))
            .expect("read workspace manifest");
        assert!(workspace.contains("packages:\n  - packages/*"));
        assert!(workspace.contains("- foo@1.0.0 || 2.0.0"));
        assert!(workspace.contains("- bar@3.0.0"));

        let actions: Vec<PromptAction> = EVENTS
            .lock()
            .expect("event lock")
            .iter()
            .filter_map(|event| match event {
                LogEvent::Prompt(log) => Some(log.action),
                _ => None,
            })
            .collect();
        assert_eq!(actions, [PromptAction::Start, PromptAction::End]);
    }

    #[test]
    fn denying_approval_leaves_the_workspace_manifest_unchanged() {
        let _test_guard = TEST_LOCK.lock().expect("test lock");
        EVENTS.lock().expect("event lock").clear();
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("pnpm-workspace.yaml");
        fs::write(&path, "packages:\n  - packages/*\n").expect("write workspace manifest");
        let original = fs::read_to_string(&path).expect("read original");
        let mut config = Config::new();
        config.minimum_release_age_strict = Some(true);
        let mut prompt = FakePrompt::default();

        let error = handle_minimum_release_age_violations_with::<RecordingReporter, _>(
            &config,
            dir.path(),
            &[violation("foo", "1.0.0", "MINIMUM_RELEASE_AGE_VIOLATION")],
            true,
            &mut prompt,
        )
        .expect_err("denied approval must abort");

        assert!(matches!(error, MinimumReleaseAgeError::Denied));
        assert_eq!(fs::read_to_string(path).expect("read unchanged manifest"), original);
        let actions: Vec<PromptAction> = EVENTS
            .lock()
            .expect("event lock")
            .iter()
            .filter_map(|event| match event {
                LogEvent::Prompt(log) => Some(log.action),
                _ => None,
            })
            .collect();
        assert_eq!(actions, [PromptAction::Start, PromptAction::End]);
    }

    #[test]
    fn prompt_input_error_releases_the_reporter() {
        let _test_guard = TEST_LOCK.lock().expect("test lock");
        EVENTS.lock().expect("event lock").clear();
        let dir = tempdir().expect("temp dir");
        let mut config = Config::new();
        config.minimum_release_age_strict = Some(true);

        let error = handle_minimum_release_age_violations_with::<RecordingReporter, _>(
            &config,
            dir.path(),
            &[violation("foo", "1.0.0", "MINIMUM_RELEASE_AGE_VIOLATION")],
            true,
            &mut FailingPrompt,
        )
        .expect_err("prompt input failure must abort");

        assert!(matches!(error, MinimumReleaseAgeError::Prompt(_)));
        let actions: Vec<PromptAction> = EVENTS
            .lock()
            .expect("event lock")
            .iter()
            .filter_map(|event| match event {
                LogEvent::Prompt(log) => Some(log.action),
                _ => None,
            })
            .collect();
        assert_eq!(actions, [PromptAction::Start, PromptAction::End]);
    }
}
