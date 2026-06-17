use super::{
    PACQUET_VERSION, default_cache_dir, default_child_concurrency,
    default_child_concurrency_with_parallelism, default_config_dir, default_fetch_timeout,
    default_store_dir, default_unsafe_perm, default_user_agent, default_workspace_concurrency,
    is_unsafe_perm_posix, resolve_child_concurrency, resolve_child_concurrency_with_parallelism,
};
use crate::api::{EnvVar, GetCurrentDir, GetHomeDir};
use pacquet_store_dir::{STORE_VERSION, StoreDir};
use pretty_assertions::assert_eq;
use std::{io, path::PathBuf};

#[cfg(windows)]
use super::{default_store_dir_windows, get_drive_letter};
#[cfg(windows)]
use std::path::Path;

fn display_store_dir(store_dir: &StoreDir) -> String {
    store_dir.display().to_string().replace('\\', "/")
}

/// The `home_dir` and `current_dir` capability impls call
/// `unreachable!` because the early `PNPM_HOME` return short-circuits
/// before either is consumed. Matches the worked example in
/// `pacquet/CODE_STYLE_GUIDE.md` (Dependency injection for tests):
/// satisfy the bound, document the precondition.
#[test]
fn test_default_store_dir_with_pnpm_home_env() {
    struct EnvWithPnpmHome;
    impl EnvVar for EnvWithPnpmHome {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_HOME").then(|| "/tmp/pnpm-home".to_owned())
        }
    }
    impl GetHomeDir for EnvWithPnpmHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("home_dir must not be called when PNPM_HOME is set");
        }
    }
    impl GetCurrentDir for EnvWithPnpmHome {
        fn current_dir() -> io::Result<PathBuf> {
            unreachable!("current_dir must not be called when PNPM_HOME is set");
        }
    }
    let store_dir = default_store_dir::<EnvWithPnpmHome>();
    assert_eq!(display_store_dir(&store_dir), format!("/tmp/pnpm-home/store/{STORE_VERSION}"));
}

/// The fake `Sys` here returns a value for `XDG_DATA_HOME` and `None`
/// for `PNPM_HOME` so the lookup is deterministic — no need to
/// snapshot-and-restore real process env state to neutralise a
/// developer's shell that has `PNPM_HOME` set.
#[test]
fn test_default_store_dir_with_xdg_env() {
    struct EnvWithXdgDataHome;
    impl EnvVar for EnvWithXdgDataHome {
        fn var(name: &str) -> Option<String> {
            (name == "XDG_DATA_HOME").then(|| "/tmp/xdg_data_home".to_owned())
        }
    }
    impl GetHomeDir for EnvWithXdgDataHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("home_dir must not be called when XDG_DATA_HOME is set");
        }
    }
    impl GetCurrentDir for EnvWithXdgDataHome {
        fn current_dir() -> io::Result<PathBuf> {
            unreachable!("current_dir must not be called when XDG_DATA_HOME is set");
        }
    }
    let store_dir = default_store_dir::<EnvWithXdgDataHome>();
    assert_eq!(
        display_store_dir(&store_dir),
        format!("/tmp/xdg_data_home/pnpm/store/{STORE_VERSION}"),
    );
}

/// Drive the home-dir capability with a fixed path so the assertion
/// is deterministic on any host. The `current_dir` impl stays
/// `unreachable!` because the non-Windows fall-through never consults
/// it.
#[cfg(not(windows))]
#[test]
fn test_default_store_dir_falls_back_to_home_dir() {
    struct NoEnvWithHome;
    impl EnvVar for NoEnvWithHome {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl GetHomeDir for NoEnvWithHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/test-user"))
        }
    }
    impl GetCurrentDir for NoEnvWithHome {
        fn current_dir() -> io::Result<PathBuf> {
            unreachable!("current_dir must not be called on non-Windows fall-through");
        }
    }
    let store_dir = default_store_dir::<NoEnvWithHome>();
    let expected = match std::env::consts::OS {
        "linux" => format!("/home/test-user/.local/share/pnpm/store/{STORE_VERSION}"),
        "macos" => format!("/home/test-user/Library/pnpm/store/{STORE_VERSION}"),
        other => panic!("unexpected target OS in test: {other}"),
    };
    assert_eq!(display_store_dir(&store_dir), expected);
}

/// The [`GetHomeDir`] impl is `unreachable!` because the
/// `XDG_CACHE_HOME` branch short-circuits before it is consumed.
#[test]
fn test_default_cache_dir_with_xdg_cache_home_env() {
    struct EnvWithXdgCacheHome;
    impl EnvVar for EnvWithXdgCacheHome {
        fn var(name: &str) -> Option<String> {
            (name == "XDG_CACHE_HOME").then(|| "/tmp/xdg-cache-home".to_owned())
        }
    }
    impl GetHomeDir for EnvWithXdgCacheHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("home_dir must not be called when XDG_CACHE_HOME is set");
        }
    }
    let cache_dir = default_cache_dir::<EnvWithXdgCacheHome>();
    let display = cache_dir.display().to_string().replace('\\', "/");
    assert_eq!(display, "/tmp/xdg-cache-home/pnpm");
}

/// The Windows branch needs `LOCALAPPDATA` handling that's not
/// portable here and is left to manual / CI-based verification.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn test_default_cache_dir_falls_back_to_platform_default() {
    struct NoEnvWithHome;
    impl EnvVar for NoEnvWithHome {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl GetHomeDir for NoEnvWithHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/test-user"))
        }
    }
    let cache_dir = default_cache_dir::<NoEnvWithHome>();
    let expected = if cfg!(target_os = "macos") {
        PathBuf::from("/home/test-user/Library/Caches/pnpm")
    } else {
        PathBuf::from("/home/test-user/.cache/pnpm")
    };
    assert_eq!(cache_dir, expected);
}

/// The `home_dir` impl uses `unreachable!` to document that the
/// `XDG_CONFIG_HOME` branch short-circuits home-dir resolution.
#[test]
fn test_default_config_dir_with_xdg_config_home_env() {
    struct EnvWithXdgConfigHome;
    impl EnvVar for EnvWithXdgConfigHome {
        fn var(name: &str) -> Option<String> {
            (name == "XDG_CONFIG_HOME").then(|| "/tmp/xdg-config-home".to_owned())
        }
    }
    impl GetHomeDir for EnvWithXdgConfigHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("home_dir must not be called when XDG_CONFIG_HOME is set");
        }
    }
    let config_dir =
        default_config_dir::<EnvWithXdgConfigHome>().expect("XDG_CONFIG_HOME bypasses home_dir");
    let display = config_dir.display().to_string().replace('\\', "/");
    assert_eq!(display, "/tmp/xdg-config-home/pnpm");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn test_default_config_dir_falls_back_to_platform_default() {
    struct NoEnvWithHome;
    impl EnvVar for NoEnvWithHome {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl GetHomeDir for NoEnvWithHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/test-user"))
        }
    }
    let config_dir = default_config_dir::<NoEnvWithHome>().expect("home dir supplied");
    let expected = if cfg!(target_os = "macos") {
        PathBuf::from("/home/test-user/Library/Preferences/pnpm")
    } else {
        PathBuf::from("/home/test-user/.config/pnpm")
    };
    assert_eq!(config_dir, expected);
}

/// `default_config_dir` returns `None` on missing home, whereas
/// `default_store_dir` / `default_cache_dir` panic, because the
/// global `config.yaml` is strictly optional whereas a store path
/// must always exist.
#[test]
fn test_default_config_dir_without_home_returns_none() {
    struct NoEnvNoHome;
    impl EnvVar for NoEnvNoHome {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl GetHomeDir for NoEnvNoHome {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    assert_eq!(default_config_dir::<NoEnvNoHome>(), None);
}

/// Port of upstream
/// [`'getDefaultWorkspaceConcurrency: cpu num < 4'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.test.ts#L25-L28).
#[test]
fn default_child_concurrency_with_parallelism_below_four() {
    assert_eq!(default_child_concurrency_with_parallelism(1), 1);
}

/// Port of upstream
/// [`'getDefaultWorkspaceConcurrency: cpu num > 4'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.test.ts#L30-L33).
#[test]
fn default_child_concurrency_with_parallelism_above_four() {
    assert_eq!(default_child_concurrency_with_parallelism(5), 4);
}

/// Port of upstream
/// [`'getDefaultWorkspaceConcurrency: cpu num = 4'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.test.ts#L35-L38).
#[test]
fn default_child_concurrency_with_parallelism_at_four() {
    assert_eq!(default_child_concurrency_with_parallelism(4), 4);
}

/// `workspaceConcurrency` and `childConcurrency` default through the
/// same upstream `getDefaultWorkspaceConcurrency`, so the two pacquet
/// defaults must agree. This pins that parity so a future change to
/// one default that forgets the other fails here.
#[test]
fn default_workspace_concurrency_matches_default_child_concurrency() {
    assert_eq!(default_workspace_concurrency(), default_child_concurrency());
}

/// Port of upstream
/// [`'default workspace concurrency'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.test.ts#L48-L52).
/// Pin a `>=4` parallelism so the expectation is deterministic.
#[test]
fn resolve_child_concurrency_default_with_four_or_more_cores() {
    assert_eq!(resolve_child_concurrency_with_parallelism(None, 4), 4);
    assert_eq!(resolve_child_concurrency_with_parallelism(None, 8), 4);
}

/// Port of upstream
/// [`'match host cores amount'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.test.ts#L58-L62).
#[test]
fn resolve_child_concurrency_zero_returns_full_parallelism() {
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(0), 8), 8);
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(0), 1), 1);
}

/// Port of upstream
/// [`'host cores minus X'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.test.ts#L64-L71).
#[test]
fn resolve_child_concurrency_negative_offset_matches_upstream_formula() {
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(-1), 8), 7);
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(-1), 1), 1);
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(-9999), 8), 1);
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(-9999), 1), 1);
}

#[test]
fn resolve_child_concurrency_positive_amount() {
    assert_eq!(resolve_child_concurrency(Some(5)), 5);
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(5), 1), 5);
    assert_eq!(resolve_child_concurrency_with_parallelism(Some(5), 100), 5);
}

/// `resolve_child_concurrency(Some(i32::MIN))` must not panic.
/// A naive `(-n) as u32` overflows in debug builds when
/// `n == i32::MIN` because the negation itself overflows;
/// `unsigned_abs` is the safe path. `i32::MIN.unsigned_abs()`
/// is `2_147_483_648`, well above any plausible host
/// parallelism, so `saturating_sub` produces `0` and `.max(1)`
/// lifts to exactly `1` — assert that precise value so a wrong
/// result like `2` would still fail the test.
#[test]
fn resolve_child_concurrency_handles_i32_min() {
    let result = resolve_child_concurrency(Some(i32::MIN));
    assert_eq!(result, 1);
}

#[test]
fn is_unsafe_perm_posix_truth_table() {
    assert!(!is_unsafe_perm_posix(0), "running as root → drop perms");
    assert!(is_unsafe_perm_posix(1), "non-root uid 1 → no drop");
    assert!(is_unsafe_perm_posix(501), "non-root uid 501 → no drop");
    assert!(is_unsafe_perm_posix(65534), "non-root uid 65534 → no drop");
}

#[cfg(windows)]
#[test]
fn default_unsafe_perm_on_windows_is_always_true() {
    assert!(default_unsafe_perm(), "Windows default must always be true");
}

/// Test environments don't usually run as root, so the per-uid
/// logic is pinned without needing root privileges by the
/// `is_unsafe_perm_posix_truth_table` test above. Cygwin is excluded
/// because `default_unsafe_perm` short-circuits to `true` on Cygwin
/// regardless of uid.
#[cfg(all(unix, not(target_os = "cygwin")))]
#[test]
fn default_unsafe_perm_on_posix_matches_runtime_uid() {
    // SAFETY: `libc::getuid` is documented as always-safe.
    let uid = unsafe { libc::getuid() } as u32;
    assert_eq!(default_unsafe_perm(), is_unsafe_perm_posix(uid));
}

#[cfg(target_os = "cygwin")]
#[test]
fn default_unsafe_perm_on_cygwin_is_always_true() {
    assert!(default_unsafe_perm(), "Cygwin default must always be true (matches upstream)");
}

#[cfg(windows)]
#[test]
fn test_should_get_the_correct_drive_letter() {
    let current_dir = Path::new("C:\\Users\\user\\project");
    let drive_letter = get_drive_letter(current_dir);
    assert_eq!(drive_letter, Some('C'));
}

#[cfg(windows)]
#[test]
fn test_default_store_dir_with_windows_diff_drive() {
    let current_dir = Path::new("D:\\Users\\user\\project");
    let home_dir = Path::new("C:\\Users\\user");

    let store_dir = default_store_dir_windows(home_dir, current_dir);
    assert_eq!(store_dir, Path::new("D:\\.pnpm-store"));
}

#[cfg(windows)]
#[test]
fn test_dynamic_default_store_dir_with_windows_same_drive() {
    let current_dir = Path::new("C:\\Users\\user\\project");
    let home_dir = Path::new("C:\\Users\\user");

    let store_dir = default_store_dir_windows(home_dir, current_dir);
    assert_eq!(store_dir, Path::new("C:\\Users\\user\\AppData\\Local\\pnpm\\store"));
}

#[test]
fn fetch_timeout_default_matches_pnpm() {
    assert_eq!(default_fetch_timeout(), 60_000);
}

/// The exact platform / arch depend on where the test runs, so assert
/// the `pnpm/pacquet-<version> npm/? node/? ` prefix and the two
/// trailing space-separated tokens rather than the full string.
#[test]
fn user_agent_default_matches_pnpm_format() {
    let ua = default_user_agent();
    let prefix = format!("pnpm/pacquet-{PACQUET_VERSION} npm/? node/? ");
    assert!(ua.starts_with(&prefix), "user-agent {ua:?} must start with {prefix:?}");
    let tail: Vec<&str> = ua[prefix.len()..].split(' ').collect();
    assert_eq!(tail.len(), 2, "expected `<platform> <arch>` tail, got {ua:?}");
    assert!(tail.iter().all(|token| !token.is_empty()), "platform/arch must be non-empty: {ua:?}");
}
