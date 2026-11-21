// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{
        Path,
        PathBuf,
    },
    rc::Rc,
};

use deno_task_shell::{
    KillSignal,
    ShellCommand,
    ShellCommandContext,
    ShellPipeWriter,
    ShellState,
    execute_with_pipes,
    parser::parse,
    pipe,
};
use futures::future::LocalBoxFuture;
use pretty_assertions::assert_eq;
use tokio::task::JoinHandle;

use deno_task_shell::ExecuteResult;

type FnShellCommandExecute =
    Box<dyn Fn(ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult>>;

struct FnShellCommand(FnShellCommandExecute);

impl ShellCommand for FnShellCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        (self.0)(context)
    }
}

// Clippy is complaining about them all having `File` prefixes,
// but there might be non-file variants in the future.
#[allow(clippy::enum_variant_names)]
enum TestAssertion {
    FileExists(String),
    FileNotExists(String),
    FileTextEquals(String, String),
}

struct TempDir {
    // hold to keep it alive until drop
    _inner: tempfile::TempDir,
    cwd: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let temp_dir = tempfile::tempdir().unwrap();
        let cwd = canonicalize_path(temp_dir.path()).unwrap();
        Self { _inner: temp_dir, cwd }
    }
}

pub struct TestBuilder {
    // it is much much faster to lazily create this
    temp_dir: Option<TempDir>,
    env_vars: HashMap<String, String>,
    custom_commands: HashMap<String, Rc<dyn ShellCommand>>,
    command: String,
    kill_signal: KillSignal,
    stdin: Vec<u8>,
    expected_exit_code: i32,
    expected_stderr: String,
    expected_stdout: String,
    assertions: Vec<TestAssertion>,
}

impl TestBuilder {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let env_vars = std::env::vars()
            .map(|(key, value)| {
                // For some very strange reason, key will sometimes be cased as "Path"
                // or other times "PATH" on Windows. Since keys are case-insensitive on
                // Windows, normalize the keys to be upper case.
                if cfg!(windows) {
                    // need to normalize on windows
                    (key.to_uppercase(), value)
                } else {
                    (key, value)
                }
            })
            .collect();

        Self {
            temp_dir: None,
            env_vars,
            custom_commands: Default::default(),
            command: Default::default(),
            kill_signal: Default::default(),
            stdin: Default::default(),
            expected_exit_code: 0,
            expected_stderr: Default::default(),
            expected_stdout: Default::default(),
            assertions: Default::default(),
        }
    }

    pub fn ensure_temp_dir(&mut self) -> &mut Self {
        self.get_temp_dir();
        self
    }

    fn get_temp_dir(&mut self) -> &mut TempDir {
        if self.temp_dir.is_none() {
            self.temp_dir = Some(TempDir::new());
        }
        self.temp_dir.as_mut().unwrap()
    }

    pub fn temp_dir_path(&mut self) -> PathBuf {
        self.get_temp_dir().cwd.clone()
    }

    pub fn command(&mut self, command: &str) -> &mut Self {
        self.command = command.to_string();
        self
    }

    pub fn stdin(&mut self, stdin: &str) -> &mut Self {
        self.stdin = stdin.as_bytes().to_vec();
        self
    }

    pub fn directory(&mut self, path: &str) -> &mut Self {
        let temp_dir = self.get_temp_dir();
        fs::create_dir_all(temp_dir.cwd.join(path)).unwrap();
        self
    }

    pub fn env_var(&mut self, name: &str, value: &str) -> &mut Self {
        self.env_vars.insert(name.to_string(), value.to_string());
        self
    }

    pub fn kill_signal(&mut self, signal: KillSignal) -> &mut Self {
        self.kill_signal = signal;
        self
    }

    pub fn custom_command(&mut self, name: &str, execute: FnShellCommandExecute) -> &mut Self {
        self.custom_commands.insert(name.to_string(), Rc::new(FnShellCommand(execute)));
        self
    }

    pub fn file(&mut self, path: &str, text: &str) -> &mut Self {
        let temp_dir = self.get_temp_dir();
        fs::write(temp_dir.cwd.join(path), text).unwrap();
        self
    }

    pub fn assert_exit_code(&mut self, code: i32) -> &mut Self {
        self.expected_exit_code = code;
        self
    }

    pub fn assert_stderr(&mut self, output: &str) -> &mut Self {
        self.expected_stderr.push_str(output);
        self
    }

    pub fn assert_stdout(&mut self, output: &str) -> &mut Self {
        self.expected_stdout.push_str(output);
        self
    }

    pub fn assert_exists(&mut self, path: &str) -> &mut Self {
        self.ensure_temp_dir();
        self.assertions.push(TestAssertion::FileExists(path.to_string()));
        self
    }

    pub fn assert_not_exists(&mut self, path: &str) -> &mut Self {
        self.ensure_temp_dir();
        self.assertions.push(TestAssertion::FileNotExists(path.to_string()));
        self
    }

    pub fn assert_file_equals(&mut self, path: &str, file_text: &str) -> &mut Self {
        self.ensure_temp_dir();
        self.assertions.push(TestAssertion::FileTextEquals(
            path.to_string(),
            file_text.to_string(),
        ));
        self
    }

    pub async fn run(&mut self) {
        let list = parse(&self.command).unwrap();
        let cwd = if let Some(temp_dir) = &self.temp_dir {
            temp_dir.cwd.clone()
        } else {
            std::env::temp_dir()
        };
        let (stdin, mut stdin_writer) = pipe();
        stdin_writer.write_all(&self.stdin).unwrap();
        drop(stdin_writer); // prevent a deadlock by dropping the writer
        let (stdout, stdout_handle) = get_output_writer_and_handle();
        let (stderr, stderr_handle) = get_output_writer_and_handle();

        let local_set = tokio::task::LocalSet::new();
        let state = ShellState::new(
            self.env_vars
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v)))
                .collect(),
            cwd.clone(),
            self.custom_commands.drain().collect(),
            self.kill_signal.clone(),
        );
        let exit_code =
            local_set.run_until(execute_with_pipes(list, state, stdin, stdout, stderr)).await;
        let temp_dir = if let Some(temp_dir) = &self.temp_dir {
            temp_dir.cwd.display().to_string()
        } else {
            "NO_TEMP_DIR".to_string()
        };
        assert_eq!(
            stderr_handle.await.unwrap(),
            self.expected_stderr.replace("$TEMP_DIR", &temp_dir),
            "\n\nFailed for: {}",
            self.command
        );
        assert_eq!(
            stdout_handle.await.unwrap(),
            self.expected_stdout.replace("$TEMP_DIR", &temp_dir),
            "\n\nFailed for: {}",
            self.command
        );
        assert_eq!(exit_code, self.expected_exit_code, "\n\nFailed for: {}", self.command);

        for assertion in &self.assertions {
            match assertion {
                TestAssertion::FileExists(path) => {
                    assert!(
                        cwd.join(path).exists(),
                        "\n\nFailed for: {}\nExpected '{}' to exist.",
                        self.command,
                        path,
                    )
                }
                TestAssertion::FileNotExists(path) => {
                    assert!(
                        !cwd.join(path).exists(),
                        "\n\nFailed for: {}\nExpected '{}' to not exist.",
                        self.command,
                        path,
                    )
                }
                TestAssertion::FileTextEquals(path, text) => {
                    let actual_text = std::fs::read_to_string(cwd.join(path))
                        .unwrap_or_else(|err| panic!("Error reading {path}: {err}"));
                    assert_eq!(
                        &actual_text, text,
                        "\n\nFailed for: {}\nPath: {}",
                        self.command, path,
                    )
                }
            }
        }
    }
}

fn get_output_writer_and_handle() -> (ShellPipeWriter, JoinHandle<String>) {
    let (reader, writer) = pipe();
    let handle = reader.pipe_to_string_handle();
    (writer, handle)
}

fn canonicalize_path(path: &Path) -> std::io::Result<PathBuf> {
    Ok(deno_path_util::strip_unc_prefix(path.canonicalize()?))
}
