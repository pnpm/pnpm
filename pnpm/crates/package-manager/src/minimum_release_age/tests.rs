use std::{fs, sync::Mutex};

use pacquet_config::Config;
use pacquet_lockfile::{LockfileResolution, RegistryResolution};
use pacquet_reporter::{LogEvent, PromptAction, Reporter, SilentReporter};
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
    async fn confirm(&mut self, message: &str) -> dialoguer::Result<bool> {
        self.messages.push(message.to_string());
        Ok(self.answer)
    }
}

struct FailingPrompt;

impl ApprovalPrompt for FailingPrompt {
    async fn confirm(&mut self, _message: &str) -> dialoguer::Result<bool> {
        Err(std::io::Error::other("prompt input failed").into())
    }
}

// Per-test recording reporter. Its `Mutex<Vec<LogEvent>>` buffer is fn-local,
// so each `#[test]` captures into its own and concurrent tests never share or
// race on it. Each test names the helpers it drives, so every emitted helper is
// used and none needs a `dead_code` allow.
macro_rules! recording_reporter {
    ($($helper:ident),* $(,)?) => {
        static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

        struct RecordingReporter;
        impl Reporter for RecordingReporter {
            fn emit(event: &LogEvent) {
                EVENTS.lock().expect("event lock").push(event.clone());
            }
        }

        $( recording_reporter!(@helper $helper); )*
    };

    (@helper reset_events) => {
        fn reset_events() {
            EVENTS.lock().expect("event lock").clear();
        }
    };
    (@helper prompt_actions) => {
        fn prompt_actions() -> Vec<PromptAction> {
            EVENTS
                .lock()
                .expect("event lock")
                .iter()
                .filter_map(|event| match event {
                    LogEvent::Prompt(log) => Some(log.action),
                    _ => None,
                })
                .collect()
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `recording_reporter!` helper `",
            stringify!($unknown),
            "`; expected one of: reset_events, prompt_actions",
        ));
    };
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

#[tokio::test]
async fn non_interactive_strict_mode_reports_every_immature_pick() {
    let dir = tempdir().expect("temp dir");
    let mut config = Config::new();
    config.minimum_release_age_strict = Some(true);
    let mut prompt = FakePrompt::default();
    let violations = vec![
        violation("zeta", "2.0.0", "MINIMUM_RELEASE_AGE_VIOLATION"),
        violation("alpha", "1.0.0", "MINIMUM_RELEASE_AGE_VIOLATION"),
        violation("ignored", "3.0.0", "TRUST_DOWNGRADE"),
    ];

    let error = handle_minimum_release_age_violations_with::<SilentReporter, _>(
        &config,
        dir.path(),
        &violations,
        false,
        &mut prompt,
    )
    .await
    .expect_err("non-interactive strict mode must fail");

    assert!(matches!(error, MinimumReleaseAgeError::NoMatureMatchingVersion { .. }));
    let message = error.to_string();
    assert!(message.starts_with("2 versions do not meet the minimumReleaseAge constraint:"));
    assert!(message.find("alpha@1.0.0").unwrap() < message.find("zeta@2.0.0").unwrap());
    assert!(!message.contains("ignored"));
    assert!(prompt.messages.is_empty());
}

#[tokio::test]
async fn approval_persists_canonical_excludes_and_brackets_the_prompt() {
    recording_reporter!(reset_events, prompt_actions);
    reset_events();
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
    .await
    .expect("approval should continue");

    assert_eq!(prompt.messages.len(), 1);
    assert!(prompt.messages[0].contains("bar@3.0.0\n  foo@2.0.0"));
    let workspace = fs::read_to_string(dir.path().join("pnpm-workspace.yaml"))
        .expect("read workspace manifest");
    assert!(workspace.contains("packages:\n  - packages/*"));
    assert!(workspace.contains("- foo@1.0.0 || 2.0.0"));
    assert!(workspace.contains("- bar@3.0.0"));

    assert_eq!(prompt_actions(), [PromptAction::Start, PromptAction::End]);
}

#[tokio::test]
async fn denying_approval_leaves_the_workspace_manifest_unchanged() {
    recording_reporter!(reset_events, prompt_actions);
    reset_events();
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
    .await
    .expect_err("denied approval must abort");

    assert!(matches!(error, MinimumReleaseAgeError::Denied));
    assert_eq!(fs::read_to_string(path).expect("read unchanged manifest"), original);
    assert_eq!(prompt_actions(), [PromptAction::Start, PromptAction::End]);
}

#[tokio::test]
async fn prompt_input_error_releases_the_reporter() {
    recording_reporter!(reset_events, prompt_actions);
    reset_events();
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
    .await
    .expect_err("prompt input failure must abort");

    assert!(matches!(error, MinimumReleaseAgeError::Prompt(_)));
    assert_eq!(prompt_actions(), [PromptAction::Start, PromptAction::End]);
}
