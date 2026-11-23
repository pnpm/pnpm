use super::{
    UnexpandedWindowsEnvVar,
    ensure_windows_dir_envs_on,
    ensure_windows_home_dir_env_on,
    expand_dir_env,
};
use crate::api::EnvVar;
use pretty_assertions::assert_eq;

fn lookup(name: &str) -> Option<String> {
    match name {
        "SOME_ENV" => Some(r"C:\tools".to_owned()),
        "OUTER" => Some(r"%INNER%\apps".to_owned()),
        "INNER" => Some(r"D:\dev".to_owned()),
        "LOCAL_ROOT" => Some(r"C:\Users\me".to_owned()),
        "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
        "A" => Some("%B%".to_owned()),
        "B" => Some("%A%".to_owned()),
        "DOUBLING" => Some("%DOUBLING%%DOUBLING%".to_owned()),
        "MY-HOME" => Some(r"C:\tools".to_owned()),
        "WIDE" => Some("é".repeat(20_000)),
        "MY HOME" => Some(r"C:\tools".to_owned()),
        _ => None,
    }
}

fn expand(value: &str) -> Result<String, UnexpandedWindowsEnvVar> {
    expand_dir_env("windows", "PNPM_HOME", value, lookup)
}

#[test]
fn expands_a_nested_windows_reference() {
    assert_eq!(expand("%SOME_ENV%/pnpm").unwrap(), r"C:\tools/pnpm");
    assert_eq!(expand(r"%OUTER%\pnpm").unwrap(), r"D:\dev\apps\pnpm");
}

#[test]
fn expands_program_files_x86() {
    assert_eq!(expand(r"%ProgramFiles(x86)%\pnpm").unwrap(), r"C:\Program Files (x86)\pnpm");
}

#[test]
fn rejects_a_reference_that_stays_unexpanded() {
    let error = expand("%MISSING%/pnpm").unwrap_err();
    assert_eq!(error.variable, "PNPM_HOME");
    assert_eq!(error.reference, "%MISSING%");
}

#[test]
fn rejects_a_cycle() {
    let error = expand("%A%").unwrap_err();
    assert_eq!(error.reference, "%A%");
}

#[test]
fn rejects_a_reference_that_grows_on_every_pass() {
    let error = expand("%DOUBLING%").unwrap_err();
    assert_eq!(error.reference, "%DOUBLING%");
}

#[test]
fn measures_the_length_limit_in_utf16_code_units() {
    let expanded = expand("%WIDE%").unwrap();
    assert_eq!(expanded.chars().count(), 20_000);
    assert_eq!(expand("%WIDE%%WIDE%").unwrap_err().reference, "%WIDE%");
}

#[test]
fn expands_a_name_with_a_hyphen() {
    assert_eq!(expand(r"%MY-HOME%\pnpm").unwrap(), r"C:\tools\pnpm");
    assert_eq!(expand(r"%MISSING-HOME%\pnpm").unwrap_err().reference, "%MISSING-HOME%");
}

#[test]
fn expands_a_name_with_an_inner_space() {
    assert_eq!(expand(r"%MY HOME%\pnpm").unwrap(), r"C:\tools\pnpm");
    assert_eq!(expand(r"%MISSING HOME%\pnpm").unwrap_err().reference, "%MISSING HOME%");
}

#[test]
fn leaves_percent_text_with_surrounding_spaces_in_place() {
    assert_eq!(expand(r"C:\100% off 50%\pnpm").unwrap(), r"C:\100% off 50%\pnpm");
}

#[test]
fn leaves_a_literal_percent_in_place() {
    assert_eq!(expand(r"C:\100%\pnpm").unwrap(), r"C:\100%\pnpm");
    assert_eq!(expand("100%%").unwrap(), "100%%");
}

#[test]
fn does_not_expand_off_windows() {
    assert_eq!(
        expand_dir_env("linux", "PNPM_HOME", "%SOME_ENV%/pnpm", lookup).unwrap(),
        "%SOME_ENV%/pnpm",
    );
}

struct DirEnv;

impl EnvVar for DirEnv {
    fn var(name: &str) -> Option<String> {
        match name {
            "PNPM_HOME" => Some("%SOME_ENV%/pnpm".to_owned()),
            "SOME_ENV" => Some(r"C:\tools".to_owned()),
            "XDG_DATA_HOME" => Some(r"%MISSING%\data".to_owned()),
            _ => None,
        }
    }
}

#[test]
fn ignores_an_unused_data_home_when_pnpm_home_expands() {
    ensure_windows_dir_envs_on::<DirEnv>("windows").unwrap();
}

struct BadCache;

impl EnvVar for BadCache {
    fn var(name: &str) -> Option<String> {
        match name {
            "PNPM_HOME" => Some(r"C:\pnpm".to_owned()),
            "XDG_CACHE_HOME" => Some(r"%MISSING%\cache".to_owned()),
            _ => None,
        }
    }
}

#[test]
fn rejects_an_unexpanded_cache_home() {
    let error = ensure_windows_dir_envs_on::<BadCache>("windows").unwrap_err();
    assert_eq!(error.variable, "XDG_CACHE_HOME");
    assert_eq!(error.reference, "%MISSING%");
}

#[test]
fn skips_the_check_off_windows() {
    ensure_windows_dir_envs_on::<BadCache>("linux").unwrap();
}

#[test]
fn home_dir_check_ignores_an_unexpanded_cache_home() {
    ensure_windows_home_dir_env_on::<BadCache>("windows").unwrap();
}

struct BadLocalAppData;

impl EnvVar for BadLocalAppData {
    fn var(name: &str) -> Option<String> {
        match name {
            "LOCALAPPDATA" => Some(r"%MISSING%\AppData\Local".to_owned()),
            _ => None,
        }
    }
}

#[test]
fn home_dir_check_rejects_an_unexpanded_local_app_data_fallback() {
    let error = ensure_windows_home_dir_env_on::<BadLocalAppData>("windows").unwrap_err();
    assert_eq!(error.variable, "LOCALAPPDATA");
}

#[test]
fn pnpm_config_home_rejects_unexpanded_windows_references() {
    struct UnexpandedConfigHome;
    impl EnvVar for UnexpandedConfigHome {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_HOME").then(|| r"%MISSING%\config".to_owned())
        }
    }
    let error = ensure_windows_dir_envs_on::<UnexpandedConfigHome>("windows").unwrap_err();
    assert_eq!(error.variable, "PNPM_CONFIG_HOME");
    assert_eq!(error.reference, "%MISSING%");
}

#[test]
fn pnpm_config_home_bypasses_unused_windows_config_fallbacks() {
    struct ExpandedConfigHome;
    impl EnvVar for ExpandedConfigHome {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_HOME" => Some(r"%ROOT%\config".to_owned()),
                "ROOT" | "PNPM_HOME" | "XDG_CACHE_HOME" | "XDG_STATE_HOME" => {
                    Some(r"C:\tools".to_owned())
                }
                "XDG_CONFIG_HOME" | "LOCALAPPDATA" => Some(r"%MISSING%\config".to_owned()),
                _ => None,
            }
        }
    }
    ensure_windows_dir_envs_on::<ExpandedConfigHome>("windows").unwrap();
    assert_eq!(
        expand_dir_env("windows", "PNPM_CONFIG_HOME", r"%ROOT%\config", ExpandedConfigHome::var)
            .unwrap(),
        r"C:\tools\config",
    );
}

#[test]
fn empty_expanded_pnpm_config_home_validates_the_xdg_fallback() {
    struct EmptyConfigHome;
    impl EnvVar for EmptyConfigHome {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_HOME" => Some("%EMPTY%".to_owned()),
                "EMPTY" => Some(String::new()),
                "XDG_CONFIG_HOME" => Some(r"%MISSING%\config".to_owned()),
                _ => None,
            }
        }
    }
    let error = ensure_windows_dir_envs_on::<EmptyConfigHome>("windows").unwrap_err();
    assert_eq!(error.variable, "XDG_CONFIG_HOME");
}

#[test]
fn an_empty_pnpm_home_validates_the_data_home_fallback() {
    struct EmptyPnpmHome;
    impl EnvVar for EmptyPnpmHome {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_HOME" => Some(String::new()),
                "XDG_DATA_HOME" => Some(r"%MISSING%\data".to_owned()),
                _ => None,
            }
        }
    }
    let error = ensure_windows_home_dir_env_on::<EmptyPnpmHome>("windows").unwrap_err();
    assert_eq!(error.variable, "XDG_DATA_HOME");
}
