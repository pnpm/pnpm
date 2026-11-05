#[cfg(unix)]
use crate::spawning;
use crate::{Child, ExitStatus, Output};
use std::{ffi::OsStr, io, ops::Deref, path::Path, process::Stdio};

pub struct Command {
    inner: std::process::Command,
    stdio: [bool; 3],
}

impl Command {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self { inner: std::process::Command::new(program), stdio: [false; 3] }
    }

    pub fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.inner.arg(argument);
        self
    }

    pub fn args<Arguments, Argument>(&mut self, arguments: Arguments) -> &mut Self
    where
        Arguments: IntoIterator<Item = Argument>,
        Argument: AsRef<OsStr>,
    {
        self.inner.args(arguments);
        self
    }

    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.inner.env(name, value);
        self
    }

    pub fn envs<Variables, Key, Value>(&mut self, variables: Variables) -> &mut Self
    where
        Variables: IntoIterator<Item = (Key, Value)>,
        Key: AsRef<OsStr>,
        Value: AsRef<OsStr>,
    {
        self.inner.envs(variables);
        self
    }

    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.inner.env_remove(name);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.inner.env_clear();
        self
    }

    pub fn current_dir(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.inner.current_dir(path);
        self
    }

    pub fn stdin(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[0] = true;
        self.inner.stdin(stream);
        self
    }

    pub fn stdout(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[1] = true;
        self.inner.stdout(stream);
        self
    }

    pub fn stderr(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[2] = true;
        self.inner.stderr(stream);
        self
    }

    #[cfg(unix)]
    pub fn process_group(&mut self, process_group: i32) -> &mut Self {
        use std::os::unix::process::CommandExt;

        self.inner.process_group(process_group);
        self
    }

    #[cfg(windows)]
    pub fn creation_flags(&mut self, flags: u32) -> &mut Self {
        use std::os::windows::process::CommandExt;

        self.inner.creation_flags(flags);
        self
    }

    #[cfg(unix)]
    pub fn exec(&mut self) -> io::Error {
        use std::os::unix::process::CommandExt;

        self.inner.exec()
    }

    #[cfg(windows)]
    pub fn raw_arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        use std::os::windows::process::CommandExt;

        self.inner.raw_arg(argument);
        self
    }

    pub fn spawn(&mut self) -> io::Result<Child> {
        #[cfg(unix)]
        let _spawning = spawning();
        self.spawn_unlocked()
    }

    pub(crate) fn spawn_unlocked(&mut self) -> io::Result<Child> {
        self.inner.spawn()
    }

    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.spawn()?.wait()
    }

    pub fn output(&mut self) -> io::Result<Output> {
        if !self.stdio[0] {
            self.inner.stdin(Stdio::null());
        }
        if !self.stdio[1] {
            self.inner.stdout(Stdio::piped());
        }
        if !self.stdio[2] {
            self.inner.stderr(Stdio::piped());
        }
        let child = self.spawn();
        self.restore_output_stdio();
        child?.wait_with_output()
    }

    fn restore_output_stdio(&mut self) {
        if !self.stdio[0] {
            self.inner.stdin(Stdio::inherit());
        }
        if !self.stdio[1] {
            self.inner.stdout(Stdio::inherit());
        }
        if !self.stdio[2] {
            self.inner.stderr(Stdio::inherit());
        }
    }
}

impl Deref for Command {
    type Target = std::process::Command;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct AsyncCommand {
    inner: tokio::process::Command,
    stdio: [bool; 3],
}

impl AsyncCommand {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self { inner: tokio::process::Command::new(program), stdio: [false; 3] }
    }

    pub fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.inner.arg(argument);
        self
    }

    pub fn args<Arguments, Argument>(&mut self, arguments: Arguments) -> &mut Self
    where
        Arguments: IntoIterator<Item = Argument>,
        Argument: AsRef<OsStr>,
    {
        self.inner.args(arguments);
        self
    }

    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.inner.env(name, value);
        self
    }

    pub fn envs<Variables, Key, Value>(&mut self, variables: Variables) -> &mut Self
    where
        Variables: IntoIterator<Item = (Key, Value)>,
        Key: AsRef<OsStr>,
        Value: AsRef<OsStr>,
    {
        self.inner.envs(variables);
        self
    }

    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.inner.env_remove(name);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.inner.env_clear();
        self
    }

    pub fn current_dir(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.inner.current_dir(path);
        self
    }

    pub fn stdin(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[0] = true;
        self.inner.stdin(stream);
        self
    }

    pub fn stdout(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[1] = true;
        self.inner.stdout(stream);
        self
    }

    pub fn stderr(&mut self, stream: impl Into<Stdio>) -> &mut Self {
        self.stdio[2] = true;
        self.inner.stderr(stream);
        self
    }

    #[cfg(unix)]
    pub fn process_group(&mut self, process_group: i32) -> &mut Self {
        self.inner.process_group(process_group);
        self
    }

    #[cfg(windows)]
    pub fn creation_flags(&mut self, flags: u32) -> &mut Self {
        self.inner.creation_flags(flags);
        self
    }

    #[cfg(unix)]
    /// # Safety
    ///
    /// The function must be safe to run in the child process after `fork` and before `exec`.
    pub unsafe fn pre_exec<Function>(&mut self, function: Function) -> &mut Self
    where
        Function: FnMut() -> io::Result<()> + Send + Sync + 'static,
    {
        unsafe {
            // SAFETY: the caller guarantees the function is safe to run between `fork` and `exec`.
            self.inner.pre_exec(function)
        };
        self
    }

    pub fn kill_on_drop(&mut self, enabled: bool) -> &mut Self {
        self.inner.kill_on_drop(enabled);
        self
    }

    pub fn spawn(&mut self) -> io::Result<tokio::process::Child> {
        #[cfg(unix)]
        let _spawning = spawning();
        self.inner.spawn()
    }

    pub async fn status(&mut self) -> io::Result<ExitStatus> {
        self.spawn()?.wait().await
    }

    pub async fn output(&mut self) -> io::Result<Output> {
        if !self.stdio[0] {
            self.inner.stdin(Stdio::null());
        }
        if !self.stdio[1] {
            self.inner.stdout(Stdio::piped());
        }
        if !self.stdio[2] {
            self.inner.stderr(Stdio::piped());
        }
        let child = self.spawn();
        self.restore_output_stdio();
        child?.wait_with_output().await
    }

    fn restore_output_stdio(&mut self) {
        if !self.stdio[0] {
            self.inner.stdin(Stdio::inherit());
        }
        if !self.stdio[1] {
            self.inner.stdout(Stdio::inherit());
        }
        if !self.stdio[2] {
            self.inner.stderr(Stdio::inherit());
        }
    }
}

impl Deref for AsyncCommand {
    type Target = tokio::process::Command;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(all(test, unix))]
#[path = "native_command_tests.rs"]
mod tests;
