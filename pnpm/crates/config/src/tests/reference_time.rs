use super::{
    Config,
    EnvVar,
    EnvVarOs,
    GetCurrentDir,
    GetHomeDir,
    HostNoHome,
    LinkProbe,
    OsString,
    Path,
    PathBuf,
    assert_eq,
    config_from_workspace_yaml,
    fs,
    io,
    safe_host_var,
    tempdir,
};
use crate::{
    TrustPolicy,
    workspace_yaml::LoadWorkspaceYamlError,
};
use jiff::Timestamp;

fn time(value: &str) -> Timestamp {
    pnpm_time::parse_rfc3339(value).expect("valid timestamp")
}

fn load_error(yaml: &str) -> LoadWorkspaceYamlError {
    let tmp = tempdir().expect("workspace tempdir");
    fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml).expect("write to pnpm-workspace.yaml");
    Config::new().current::<HostNoHome>(tmp.path()).expect_err("the value is invalid")
}

#[test]
fn reads_reference_time_from_the_workspace_yaml() {
    let config = config_from_workspace_yaml("referenceTime: '2026-01-02T03:04:05+02:00'\n");
    assert_eq!(config.reference_time, Some(time("2026-01-02T01:04:05Z")));
    assert_eq!(config.resolved_reference_time(), time("2026-01-02T01:04:05Z"));
}

#[test]
fn rejects_a_reference_time_that_is_not_a_timestamp_or_is_in_the_future() {
    for (yaml, reason) in [
        ("referenceTime: yesterday\n", "is not an RFC 3339 timestamp"),
        ("referenceTime: '2999-01-01T00:00:00Z'\n", "is later than the current time"),
    ] {
        let error = load_error(yaml);
        assert!(
            matches!(&error, LoadWorkspaceYamlError::InvalidReferenceTime { reason: got, .. } if *got == reason),
            "{yaml}: got {error:?}",
        );
    }
}

#[test]
fn reference_time_env_var_wins_over_the_workspace_yaml() {
    struct HostWithReferenceTimeEnv;
    impl EnvVar for HostWithReferenceTimeEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_REFERENCE_TIME" => Some("2026-03-01T00:00:00Z".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithReferenceTimeEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithReferenceTimeEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithReferenceTimeEnv);
    host_current_dir!(HostWithReferenceTimeEnv);

    let tmp = tempdir().expect("workspace tempdir");
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "referenceTime: '2026-01-01T00:00:00Z'\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostWithReferenceTimeEnv>(tmp.path()).expect("loads");
    assert_eq!(config.reference_time, Some(time("2026-03-01T00:00:00Z")));
}

#[test]
fn an_invalid_reference_time_env_var_is_ignored() {
    struct HostWithBadReferenceTimeEnv;
    impl EnvVar for HostWithBadReferenceTimeEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_REFERENCE_TIME" => Some("2026-03-01".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithBadReferenceTimeEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithBadReferenceTimeEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithBadReferenceTimeEnv);
    host_current_dir!(HostWithBadReferenceTimeEnv);

    let tmp = tempdir().expect("workspace tempdir");
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "referenceTime: '2026-01-01T00:00:00Z'\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostWithBadReferenceTimeEnv>(tmp.path()).expect("loads");
    assert_eq!(config.reference_time, Some(time("2026-01-01T00:00:00Z")));
}

#[test]
fn reference_time_is_the_current_time_when_unset() {
    let before = Timestamp::now();
    let now = Config::default().resolved_reference_time();
    assert!(before <= now && now <= Timestamp::now(), "{now} is not the current time");
}

#[test]
fn accepts_a_reference_time_a_few_minutes_ahead_of_this_clock() {
    let ahead = (Timestamp::now() + jiff::SignedDuration::from_mins(5)).to_string();
    let config = config_from_workspace_yaml(&format!("referenceTime: '{ahead}'\n"));
    assert!(config.reference_time.is_some(), "clock skew of 5 minutes is allowed");
}

#[test]
fn reference_time_applies_only_when_a_window_counts_back_from_it() {
    let now = Some(time("2026-01-01T00:00:00Z"));
    let applies = |minimum_release_age, trust_policy_ignore_after, reference_time| {
        Config {
            minimum_release_age,
            reference_time,
            trust_policy: TrustPolicy::NoDowngrade,
            trust_policy_ignore_after,
            ..Config::default()
        }
        .reference_time_applies()
    };
    assert!(applies(Some(1440), None, now), "age cutoff");
    assert!(applies(Some(0), Some(60), now), "trust window");
    let trust_off = Config {
        minimum_release_age: Some(0),
        reference_time: now,
        trust_policy_ignore_after: Some(60),
        ..Config::default()
    };
    assert!(!trust_off.reference_time_applies(), "a trust window with trustPolicy off");
    assert!(!applies(Some(0), None, now), "nothing counts back from it");
    assert!(!applies(Some(1440), Some(60), None), "unset");
}

#[test]
fn release_age_window_from_now_adds_the_lag_of_reference_time() {
    let config = Config {
        minimum_release_age: Some(1440),
        reference_time: Some(Timestamp::now() - jiff::SignedDuration::from_hours(48)),
        ..Config::default()
    };
    let window = config.release_age_window_from_now().expect("age check is on");
    assert!((1440 + 2 * 1440..=1440 + 2 * 1440 + 1).contains(&window), "window {window}");
}
