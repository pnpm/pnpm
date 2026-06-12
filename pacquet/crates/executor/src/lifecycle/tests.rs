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

    // Sequence: Script (postinstall) → some Stdio events → Exit (0).
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

/// `RunPostinstallHooks.optional` is stamped into both the `Script`
/// and `Exit` `pnpm:lifecycle` events, matching upstream's
/// `lifecycleLogger.debug({ optional, … })` shape at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/lifecycle/src/runLifecycleHook.ts#L102>
/// and `:165`. The two-bit truth on the wire lets the default
/// reporter dispatch (e.g. quieting optional-dep noise) the same
/// way it does against pnpm.
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

/// Failing scripts emit a Script event, the captured stdio, and an Exit
/// event with the resolved non-zero exit code, then return a
/// [`LifecycleScriptError::ScriptFailed`].
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

/// `SilentReporter` works as the production no-op. Same script, but no
/// recording — proves the function compiles and runs under the
/// production sink without touching the wire.
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

/// Missing `package.json` is treated as "no scripts to run" — mirrors
/// upstream `safeReadPackageJsonFromDir` returning `null` on `ENOENT`
/// and `runPostinstallHooks` returning `false` for `null` packages
/// (`https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/index.ts#L22-L23`).
#[test]
fn missing_manifest_returns_false() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    // No package.json written.

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

/// End-to-end check that the spawned child sees `npm_lifecycle_event`,
/// `npm_lifecycle_script`, `INIT_CWD`, `npm_package_name`, and
/// `npm_package_version`, and does NOT see leaked `npm_config_*` keys
/// from this process's env. Adapts the upstream test at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/lifecycle/test/index.ts#L65-L77>
/// to a file-dump model so we don't need an IPC fixture.
///
/// Unix-only: relies on `printf` and `$VAR` expansion, which `cmd`
/// (the Windows default per item `#4`) doesn't speak. Env stamping
/// itself is platform-agnostic and covered by the unit tests in
/// [`crate::make_env`].
#[cfg(unix)]
#[test]
fn child_sees_stamped_npm_package_and_no_leaked_npm_config() {
    /// RAII guard that removes a process env var on drop, so an
    /// assertion failure can't leak the seed into sibling tests.
    /// Stdlib `set_var`/`remove_var` are `unsafe` in current Rust;
    /// SAFETY: nextest runs each test in its own thread, so the
    /// only risk is sibling tests calling `env::vars()`
    /// concurrently — the guard's `Drop` still runs on panic.
    struct EnvGuard(&'static str);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var(self.0) }
        }
    }
    let _guard = EnvGuard("npm_config_should_be_stripped");
    unsafe { std::env::set_var("npm_config_should_be_stripped", "leak") };

    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let dump_path = pkg_root.join("env.dump");

    let manifest = serde_json::json!({
        "name": "stamp-target",
        "version": "9.9.9",
        "config": { "myKey": "myValue" },
        "scripts": {
            // Write a handful of env vars to the dump file; using
            // printf so the line endings are deterministic across
            // shells.
            "postinstall": format!(
                "printf 'stage=%s\\nscript=%s\\nname=%s\\nver=%s\\nconfig=%s\\ninit_cwd=%s\\nleak=%s\\n' \"$npm_lifecycle_event\" \"$npm_lifecycle_script\" \"$npm_package_name\" \"$npm_package_version\" \"$npm_package_config_myKey\" \"$INIT_CWD\" \"$npm_config_should_be_stripped\" > {}",
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
        ("leak", ""), // stripped — child sees empty string
    ];
    for (k, v) in expected_pairs {
        let line = format!("{k}={v}\n");
        assert!(dump.contains(&line), "missing line {line:?} in dump:\n{dump}");
    }
    // `script=` line contains the actual script body; just check the
    // key is there with the printf prefix.
    assert!(dump.contains("script=printf"), "missing script= line in dump:\n{dump}");
}

/// Malformed `package.json` surfaces as a `ReadManifest` error wrapping
/// `PackageManifestError::Serialization`. Mirrors upstream which throws
/// `BAD_PACKAGE_JSON` from `readPackageJson` and lets it propagate
/// through `safeReadPackageJsonFromDir` (only `ENOENT` is swallowed) at
/// `https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manifest/reader/src/index.ts#L20-L46`.
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
