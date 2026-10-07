use super::{RunScript, ScriptOutput, parsed_by_cmd, run_script};
use crate::{extend_path::ScriptsPrependNodePath, script_exit::ScriptExit};
use pnpm_reporter::{LifecycleMessage, LogEvent};
use std::{collections::HashMap, fs, path::Path, sync::Mutex};
use tempfile::tempdir;

#[test]
fn only_cmd_parses_extra_arguments() {
    assert!(parsed_by_cmd(false, true));
    assert!(!parsed_by_cmd(true, true));
    assert!(!parsed_by_cmd(false, false));
    assert!(!parsed_by_cmd(true, false));
}

const ARGS_THAT_CMD_INTERPRETS: [&str; 8] = [
    r"C:\Program Files\tool\",
    "%PATH%",
    "a b",
    r#"a"b"#,
    "tab\there",
    "^&|<>()!",
    "",
    r"ends with a backslash\",
];

/// A project whose `echo.js` records its arguments in `output.json`, with a
/// `node_modules/.bin/record-args` shim that runs it.
fn project_recording_args() -> tempfile::TempDir {
    let dir = tempdir().expect("temp dir");
    let echo = "require('fs').writeFileSync(require('path').join(__dirname, 'output.json'), \
                JSON.stringify(process.argv.slice(2)))";
    fs::write(dir.path().join("echo.js"), echo).expect("write echo.js");
    let bin_dir = dir
        .path()
        .join("node_modules")
        .join(".bin");
    fs::create_dir_all(&bin_dir).expect("create the bin dir");
    fs::write(bin_dir.join("record-args.cmd"), "@node \"%~dp0\\..\\..\\echo.js\" %*\r\n")
        .expect("write the cmd shim");
    let sh_shim = bin_dir.join("record-args");
    fs::write(&sh_shim, "#!/bin/sh\nexec node \"$(dirname \"$0\")/../../echo.js\" \"$@\"\n")
        .expect("write the sh shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sh_shim, fs::Permissions::from_mode(0o755))
            .expect("make the sh shim executable");
    }
    dir
}

#[test]
fn run_script_passes_the_args_unchanged() {
    let args: Vec<String> = ARGS_THAT_CMD_INTERPRETS
        .iter()
        .map(ToString::to_string)
        .collect();
    for script in ["node echo.js", "record-args"] {
        let dir = project_recording_args();
        let status = run(dir.path(), "echo", script, &args);
        assert!(status.success(), "`{script}` should exit cleanly");
        let recorded: Vec<String> = serde_json::from_str(
            &fs::read_to_string(dir.path().join("output.json")).expect("read output.json"),
        )
        .expect("parse output.json");
        assert_eq!(recorded, args, "the arguments `{script}` received");
    }
}

#[test]
fn run_script_shows_the_args_quoted_the_posix_way() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    fn record(event: &LogEvent) {
        EVENTS
            .lock()
            .expect("lock")
            .push(event.clone());
    }

    let dir = project_recording_args();
    let args = ["a b".to_string(), "%PATH%".to_string()];
    let invocation = crate::ScriptInvocation { stage: "echo", script: "node echo.js", args: &args };
    let output = ScriptOutput::Streamed { dep_path: "project", emit: record, color: false };
    assert!(run_with_output(dir.path(), invocation, output, &HashMap::new()).success());

    let shown: Vec<String> = EVENTS
        .lock()
        .expect("lock")
        .iter()
        .filter_map(|event| match event {
            LogEvent::Lifecycle(log) => match &log.message {
                LifecycleMessage::Script { script, .. } => Some(script.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(shown, ["node echo.js 'a b' %PATH%"]);
}

/// Run a streamed script that prints its `FORCE_COLOR`, with color
/// requested and `extra_env` set for the script.
fn streamed_force_color(extra_env: &HashMap<String, String>, emit: fn(&LogEvent)) {
    let dir = tempdir().expect("temp dir");
    let invocation = crate::ScriptInvocation {
        stage: "color",
        script: r#"node -e "console.log(process.env.FORCE_COLOR)""#,
        args: &[],
    };
    let output = ScriptOutput::Streamed { dep_path: "project", emit, color: true };
    assert!(run_with_output(dir.path(), invocation, output, extra_env).success());
}

fn stdout_lines(events: &Mutex<Vec<LogEvent>>) -> Vec<String> {
    events
        .lock()
        .expect("lock")
        .iter()
        .filter_map(|event| match event {
            LogEvent::Lifecycle(log) => match &log.message {
                LifecycleMessage::Stdio { line, .. } => Some(line.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn streamed_script_with_color_gets_force_color() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    fn record(event: &LogEvent) {
        EVENTS
            .lock()
            .expect("lock")
            .push(event.clone());
    }

    if std::env::var_os("FORCE_COLOR").is_some() {
        // The child inherits the runner's value, which takes precedence.
        return;
    }
    streamed_force_color(&HashMap::new(), record);
    assert_eq!(stdout_lines(&EVENTS), ["1"]);
}

#[test]
fn streamed_script_keeps_a_configured_force_color() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    fn record(event: &LogEvent) {
        EVENTS
            .lock()
            .expect("lock")
            .push(event.clone());
    }

    let extra_env = HashMap::from([("FORCE_COLOR".to_string(), "3".to_string())]);
    streamed_force_color(&extra_env, record);
    assert_eq!(stdout_lines(&EVENTS), ["3"]);
}

fn manifest() -> serde_json::Value {
    serde_json::json!({ "name": "t", "version": "1.0.0" })
}

fn run(pkg_root: &Path, stage: &str, script: &str, args: &[String]) -> ScriptExit {
    run_with_output(
        pkg_root,
        crate::ScriptInvocation { stage, script, args },
        ScriptOutput::Inherit,
        &HashMap::new(),
    )
}

fn run_with_output(
    pkg_root: &Path,
    invocation: crate::ScriptInvocation<'_>,
    output: ScriptOutput<'_>,
    extra_env: &HashMap<String, String>,
) -> ScriptExit {
    run_script(&RunScript {
        environment: crate::ScriptEnvironment {
            init_cwd: pkg_root,
            node_execpath: None,
            npm_execpath: None,
            node_gyp_path: None,
            user_agent: None,
            extra_env,
        },
        execution: crate::ScriptExecutionOptions {
            extra_bin_paths: &[],
            node_gyp_bin: None,
            prepend_node_path: ScriptsPrependNodePath::Never,
            shell: None,
            shell_emulator: false,
            wd_bin_dir: None,
        },
        invocation,
        manifest: &manifest(),

        pkg_root,

        silent: true,
        output,
        process_tracker: None,
    })
    .expect("run the script")
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell script body")]
fn run_script_stamps_npm_lifecycle_event() {
    let dir = tempdir().expect("temp dir");
    let marker = dir.path().join("stage.txt");
    let script = format!(r#"printf %s "$npm_lifecycle_event" > "{}""#, marker.display());

    let status = run(dir.path(), "build", &script, &[]);
    assert!(status.success(), "the script should exit cleanly");
    let written = fs::read_to_string(&marker).expect("read marker");
    assert_eq!(written, "build");
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell script body")]
fn run_script_prepends_node_modules_bin_to_path() {
    let dir = tempdir().expect("temp dir");
    let marker = dir.path().join("path.txt");
    let script = format!(r#"printf %s "$PATH" > "{}""#, marker.display());

    run(dir.path(), "build", &script, &[]);
    let written = fs::read_to_string(&marker).expect("read marker");
    let expected_bin = dir
        .path()
        .join("node_modules")
        .join(".bin");
    eprintln!("PATH:\n{written}\n");
    assert!(
        written
            .split(':')
            .any(|entry| Path::new(entry) == expected_bin),
        "PATH should contain the project's node_modules/.bin",
    );
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell script body")]
fn run_script_returns_the_scripts_exit_status() {
    let dir = tempdir().expect("temp dir");
    let status = run(dir.path(), "build", "exit 7", &[]);
    assert_eq!(status.code(), Some(7));
}
