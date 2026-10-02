use crate::{Child, ExitStatus, Output, child::AsyncChild, host};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap, ffi::OsStr, fs::File, io, marker::PhantomData, os::wasi::io::AsRawFd,
    path::Path,
};

#[derive(Debug, Default)]
pub enum Stdio {
    #[default]
    Inherit,
    Null,
    Piped,
    File(File),
}

impl Stdio {
    #[must_use]
    pub fn inherit() -> Self {
        Self::Inherit
    }
    #[must_use]
    pub fn null() -> Self {
        Self::Null
    }
    #[must_use]
    pub fn piped() -> Self {
        Self::Piped
    }
    fn to_host(&self) -> Value {
        match self {
            Self::Inherit => json!("inherit"),
            Self::Null => json!("ignore"),
            Self::Piped => json!("pipe"),
            Self::File(file) => json!({"fd":file.as_raw_fd()}),
        }
    }
}

impl From<File> for Stdio {
    fn from(file: File) -> Self {
        Self::File(file)
    }
}

#[derive(Debug)]
pub struct Blocking;
#[derive(Debug)]
pub struct Async;

#[derive(Debug)]
pub struct Command<Mode> {
    options: std::process::Command,
    stdio: [Option<Stdio>; 3],
    env_clear: bool,
    shell_emulator: bool,
    kill_on_drop: bool,
    mode: PhantomData<Mode>,
}

impl<Mode> Command<Mode> {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            options: std::process::Command::new(program),
            stdio: [None, None, None],
            env_clear: false,
            shell_emulator: false,
            kill_on_drop: false,
            mode: PhantomData,
        }
    }
    pub fn shell_emulator(script: &str) -> Self {
        let mut command = Self::new(script);
        command.shell_emulator = true;
        command
    }
    pub fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.options.arg(argument);
        self
    }
    pub fn args<I, S>(&mut self, arguments: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.options.args(arguments);
        self
    }
    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.options.env(name, value);
        self
    }
    pub fn envs<I, K, V>(&mut self, variables: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.options.envs(variables);
        self
    }
    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.options.env_remove(name);
        self
    }
    pub fn env_clear(&mut self) -> &mut Self {
        self.options.env_clear();
        self.env_clear = true;
        self
    }
    pub fn current_dir(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.options.current_dir(path);
        self
    }
    pub fn stdin(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[0] = Some(stream.into());
        self
    }
    pub fn stdout(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[1] = Some(stream.into());
        self
    }
    pub fn stderr(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[2] = Some(stream.into());
        self
    }
    pub fn kill_on_drop(&mut self, enabled: bool) -> &mut Self {
        self.kill_on_drop = enabled;
        self
    }
    pub fn get_program(&self) -> &OsStr {
        self.options.get_program()
    }
    pub fn get_args(&self) -> std::process::CommandArgs<'_> {
        self.options.get_args()
    }
    pub fn get_envs(&self) -> std::process::CommandEnvs<'_> {
        self.options.get_envs()
    }
    pub fn get_current_dir(&self) -> Option<&Path> {
        self.options.get_current_dir()
    }

    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "spawning requires exclusive command access, matching std and Tokio"
    )]
    fn spawn_child(&mut self, capture: bool) -> io::Result<Child> {
        let stdio = self.stdio
            .iter()
            .enumerate()
            .map(|(index, stream)| {
                stream.as_ref().map_or_else(|| default_stdio(capture, index), Stdio::to_host)
            })
            .collect::<Vec<_>>();
        let message = json!({
            "operation":if self.shell_emulator { "shell.spawn" } else { "process.spawn" },
            "program":text(self.options.get_program())?,
            "script":self.shell_emulator.then(|| text(self.options.get_program())).transpose()?,
            "args":self.options.get_args().map(text).collect::<io::Result<Vec<_>>>()?,
            "cwd":self.working_directory()?,
            "env":self.environment()?, "stdin":stdio[0], "stdout":stdio[1], "stderr":stdio[2],
        });
        let response = host::request(&message)?;
        Child::from_host(response, stdio[0] == "pipe")
    }

    fn working_directory(&self) -> io::Result<String> {
        let current = std::env::current_dir()?;
        let directory = match self.options.get_current_dir() {
            Some(path) => current.join(path),
            None => current,
        };
        text(directory.as_os_str()).map(str::to_owned)
    }

    fn environment(&self) -> io::Result<BTreeMap<String, String>> {
        let mut environment = if self.env_clear {
            BTreeMap::new()
        } else {
            std::env::vars_os()
                .map(|(key, value)| Ok((text(&key)?.to_owned(), text(&value)?.to_owned())))
                .collect::<io::Result<_>>()?
        };
        for (key, value) in self.options.get_envs() {
            match value {
                Some(value) => {
                    environment.insert(text(key)?.to_owned(), text(value)?.to_owned());
                }
                None => {
                    environment.remove(text(key)?);
                }
            }
        }
        Ok(environment)
    }
}

impl Command<Blocking> {
    pub fn spawn(&mut self) -> io::Result<Child> {
        self.spawn_child(false)
    }
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.spawn()?.wait()
    }
    pub fn output(&mut self) -> io::Result<Output> {
        self.spawn_child(true)?.wait_with_output()
    }
}

impl Command<Async> {
    pub fn spawn(&mut self) -> io::Result<AsyncChild> {
        Ok(AsyncChild::new(self.spawn_child(false)?, self.kill_on_drop))
    }
    pub async fn status(&mut self) -> io::Result<ExitStatus> {
        self.spawn()?.wait().await
    }
    pub async fn output(&mut self) -> io::Result<Output> {
        AsyncChild::new(self.spawn_child(true)?, self.kill_on_drop).wait_with_output().await
    }
}

fn text(value: &OsStr) -> io::Result<&str> {
    value
        .to_str()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "WebContainer process arguments must be valid UTF-8",
            )
        })
}

fn default_stdio(capture: bool, index: usize) -> Value {
    match (capture, index) {
        (true, 0) => json!("ignore"),
        (true, _) => json!("pipe"),
        (false, _) => json!("inherit"),
    }
}
