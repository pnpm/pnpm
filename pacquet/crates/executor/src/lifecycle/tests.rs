use super::{LifecycleScriptError, RunPostinstallHooks, run_postinstall_hooks};
use crate::extend_path::ScriptsPrependNodePath;
use pacquet_package_manifest::PackageManifestError;
use pacquet_reporter::{LifecycleMessage, LogEvent, Reporter, SilentReporter};
#[cfg(unix)]
use pacquet_reporter::{LifecycleStdio, LogLevel};
#[cfg(unix)]
use pretty_assertions::assert_eq;
use std::{collections::HashMap, fs, sync::Mutex};
use tempfile::tempdir;

/// Recording-fake reporter that pushes every emitted [`LogEvent`] into
/// `EVENTS`. The static lives in this test function's own scope, so
/// other tests have independent buffers.
///
/// Unix-only: the script body uses `;` and `1>&2`, which `cmd /d /s /c`
/// (the default shell pacquet now picks on Windows, per item `#4`)
/// does not interpret the same way. Windows e2e coverage for
/// lifecycle spawning is a follow-up — for now the cmd path is
/// exercised by the unit tests in [`crate::shell`].
#[cfg(unix)]
#[test]
fn lifecycle_emits_script_stdio_and_exit_in_order() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let manifest = serde_json::json!({
        "name": "x",
        "version": "1.0.0",
        "scripts": { "postinstall": "echo HELLO; echo BAD 1>&2" },
    });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/x@1.0.0",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: false,
    };

    let ran = run_postinstall_hooks::<RecordingReporter>(&opts).expect("postinstall");
    assert!(ran, "postinstall script should report executed");

    let captured = EVENTS.lock().expect("lock").clone();
    dbg!(&captured);

    let first = captured.first().expect("at least one event");
    let LogEvent::Lifecycle(first) = first else {
        panic!("first event must be Lifecycle, got {first:?}");
    };
    assert_eq!(first.level, LogLevel::Debug);
    assert!(
        matches!(
            &first.message,
            LifecycleMessage::Script { dep_path, stage, script, .. }
                if dep_path == "/x@1.0.0"
                && stage == "postinstall"
                && script.contains("echo HELLO"),
        ),
        "first event must be Script(postinstall): {first:?}",
    );

    let last = captured.last().expect("at least one event");
    let LogEvent::Lifecycle(last) = last else {
        panic!("last event must be Lifecycle, got {last:?}");
    };
    assert!(
        matches!(
            &last.message,
            LifecycleMessage::Exit { dep_path, exit_code, stage, .. }
                if dep_path == "/x@1.0.0" && *exit_code == 0 && stage == "postinstall",
        ),
        "last event must be Exit(0): {last:?}",
    );

    // Stdio events between Script and Exit. Match by line content rather
    // than by index because the order between stdout and stderr is
    // race-y (each pumps from its own thread).
    let stdio: Vec<_> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::Lifecycle(l) => match &l.message {
                LifecycleMessage::Stdio { line, stdio, .. } => Some((stdio, line.as_str())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    dbg!(&stdio);
    assert!(
        stdio.iter().any(|(s, l)| **s == LifecycleStdio::Stdout && *l == "HELLO"),
        "stdout 'HELLO' must be emitted: {stdio:?}",
    );
    assert!(
        stdio.iter().any(|(s, l)| **s == LifecycleStdio::Stderr && *l == "BAD"),
        "stderr 'BAD' must be emitted: {stdio:?}",
    );
}

#[cfg(unix)]
#[test]
fn lifecycle_events_carry_optional_flag() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let manifest = serde_json::json!({
        "name": "opt",
        "version": "1.0.0",
        "scripts": { "postinstall": "true" },
    });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/opt@1.0.0",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: true,
    };

    run_postinstall_hooks::<RecordingReporter>(&opts).expect("postinstall");

    let captured = EVENTS.lock().expect("lock").clone();
    let lifecycle_events: Vec<_> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::Lifecycle(l) => Some(&l.message),
            _ => None,
        })
        .collect();
    dbg!(&lifecycle_events);
    let script_optional = lifecycle_events
        .iter()
        .find_map(|message| match message {
            LifecycleMessage::Script { optional, .. } => Some(*optional),
            _ => None,
        })
        .expect("must emit a Script event");
    assert!(script_optional, "Script event must carry optional=true");
    let exit_optional = lifecycle_events
        .iter()
        .find_map(|message| match message {
            LifecycleMessage::Exit { optional, .. } => Some(*optional),
            _ => None,
        })
        .expect("must emit an Exit event");
    assert!(exit_optional, "Exit event must carry optional=true");
}

#[test]
fn lifecycle_emits_exit_with_nonzero_code_on_failure() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let manifest = serde_json::json!({
        "name": "y",
        "version": "1.0.0",
        "scripts": { "postinstall": "exit 7" },
    });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/y@1.0.0",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: false,
    };

    let err = run_postinstall_hooks::<RecordingReporter>(&opts).expect_err("script must fail");
    eprintln!("ERR: {err}");

    let captured = EVENTS.lock().expect("lock").clone();
    dbg!(&captured);

    let last = captured.last().expect("at least one event");
    let LogEvent::Lifecycle(last) = last else {
        panic!("last event must be Lifecycle, got {last:?}");
    };
    assert!(
        matches!(&last.message, LifecycleMessage::Exit { exit_code, .. } if *exit_code == 7),
        "last event must be Exit(7): {last:?}",
    );
}

#[test]
fn lifecycle_runs_under_silent_reporter() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let manifest = serde_json::json!({
        "name": "z",
        "version": "1.0.0",
        "scripts": { "postinstall": "echo z" },
    });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/z@1.0.0",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: false,
    };

    let ran = run_postinstall_hooks::<SilentReporter>(&opts).expect("postinstall");
    assert!(ran, "postinstall script should report executed: ran={ran}");
}

#[test]
fn missing_manifest_returns_false() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/missing@1.0.0",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: false,
    };

    let ran = run_postinstall_hooks::<SilentReporter>(&opts).expect("missing manifest is OK");
    assert!(!ran, "missing manifest must report no scripts ran: ran={ran}");
}

/// Unix-only: relies on `printf` and `$VAR` expansion, which `cmd`
/// (the Windows default per item `#4`) doesn't speak. Env stamping
/// itself is platform-agnostic and covered by the unit tests in
/// [`crate::make_env`].
#[cfg(unix)]
#[test]
fn child_sees_stamped_npm_package_and_preserves_user_config() {
    /// RAII guard that restores a process env var to its pre-test
    /// value on drop, so an assertion failure can't leak the seed
    /// into sibling tests — nor clobber a value the env already had.
    struct EnvGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }
    impl EnvGuard {
        fn new(key: &'static str) -> Self {
            EnvGuard { key, prev: std::env::var_os(key) }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: nextest runs each test in its own thread, so the
            // only risk is sibling tests calling `env::vars()`
            // concurrently — this `Drop` still runs on panic.
            unsafe {
                match self.prev.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
    let _user_guard = EnvGuard::new("npm_config_platform_arch");
    let _auth_guard = EnvGuard::new("npm_config__authtoken");
    // SAFETY: nextest runs each test in its own thread, so the only
    // risk is sibling tests calling `env::vars()` concurrently — the
    // guards' `Drop` removes the vars even on panic.
    unsafe {
        std::env::set_var("npm_config_platform_arch", "x64");
        std::env::set_var("npm_config__authtoken", "should-not-leak");
    }

    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let dump_path = pkg_root.join("env.dump");

    let manifest = serde_json::json!({
        "name": "stamp-target",
        "version": "9.9.9",
        "config": { "myKey": "myValue" },
        "scripts": {
            // printf, not echo, so the line endings are deterministic
            // across shells.
            "postinstall": format!(
                "printf 'stage=%s\\nscript=%s\\nname=%s\\nver=%s\\nconfig=%s\\ninit_cwd=%s\\nuser=%s\\nauth=%s\\n' \"$npm_lifecycle_event\" \"$npm_lifecycle_script\" \"$npm_package_name\" \"$npm_package_version\" \"$npm_package_config_myKey\" \"$INIT_CWD\" \"$npm_config_platform_arch\" \"$npm_config__authtoken\" > {}",
                dump_path.display(),
            ),
        },
    });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/stamp-target@9.9.9",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: false,
    };

    let ran = run_postinstall_hooks::<SilentReporter>(&opts).expect("postinstall");
    assert!(ran, "run_postinstall_hooks must report at least one script ran: ran={ran}");

    let dump = fs::read_to_string(&dump_path).expect("read env dump");

    let expected_init_cwd = pkg_root.to_string_lossy();
    let expected_pairs = [
        ("stage", "postinstall"),
        ("name", "stamp-target"),
        ("ver", "9.9.9"),
        ("config", "myValue"),
        ("init_cwd", expected_init_cwd.as_ref()),
        ("user", "x64"), // user-defined npm_config_* is preserved
        ("auth", ""),    // auth npm_config_* is stripped — child sees empty string
    ];
    for (k, v) in expected_pairs {
        let line = format!("{k}={v}\n");
        assert!(dump.contains(&line), "missing line {line:?} in dump:\n{dump}");
    }
    assert!(dump.contains("script=printf"), "missing script= line in dump:\n{dump}");
}

#[test]
fn malformed_manifest_propagates_error() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    fs::write(pkg_root.join("package.json"), "{ this is not valid json")
        .expect("write malformed manifest");

    let extra_env: HashMap<String, String> = HashMap::new();
    let extra_bin_paths: Vec<std::path::PathBuf> = vec![];
    let opts = RunPostinstallHooks {
        dep_path: "/malformed@1.0.0",
        pkg_root,
        root_modules_dir: pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        optional: false,
    };

    let err = run_postinstall_hooks::<SilentReporter>(&opts).expect_err("malformed JSON must fail");
    eprintln!("ERR: {err}");
    assert!(
        matches!(
            err,
            LifecycleScriptError::ReadManifest {
                source: PackageManifestError::Serialization(_),
                ..
            },
        ),
        "expected ReadManifest(Serialization), got {err:?}",
    );
}
