pub mod asynchronous {
    #[cfg(not(target_family = "wasm"))]
    pub use crate::native_command::AsyncCommand as Command;
    #[cfg(target_family = "wasm")]
    pub use crate::{ChildStderr, ChildStdin, ChildStdout, child::AsyncChild as Child};
    #[cfg(not(target_family = "wasm"))]
    pub use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
    #[cfg(target_family = "wasm")]
    pub type Command = crate::command::Command<crate::command::Async>;
}

#[cfg(target_family = "wasm")]
pub use child::{Child, Output};
#[cfg(target_family = "wasm")]
pub use command::Stdio;
#[cfg(not(target_family = "wasm"))]
pub use native_command::Command;
#[cfg(target_family = "wasm")]
pub use pipe::{ReadPipe as ChildStderr, ReadPipe as ChildStdout, WritePipe as ChildStdin};
#[cfg(target_family = "wasm")]
pub use pnpm_wasm_host::process_id as id;
#[cfg(target_family = "wasm")]
pub use signals::{HostSignal, kill, next_signal, signal};
#[cfg(target_family = "wasm")]
pub use status::ExitStatus;
#[cfg(not(target_family = "wasm"))]
pub use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, ExitStatus, Output, Stdio, id,
};
pub use std::process::{ExitCode, abort, exit};

use std::sync::{RwLock, RwLockWriteGuard};

/// Held shared by every [`Command`] spawn and exclusively by the spawn of
/// the executor's process-group watchdog.
///
/// Where `pipe2` is missing, as on macOS, Rust creates a pipe without
/// `FD_CLOEXEC` and sets it afterwards. A child forked by another thread in
/// between inherits the watchdog's lifeline and holds it open after pnpm
/// dies, so the watchdog never sees end of input and no group is killed.
/// The watchdog likewise could inherit a child's output pipe and keep pnpm
/// from seeing that child's end of output.
static SPAWNING: RwLock<()> = RwLock::new(());

/// Take [`SPAWNING`] shared for the duration of one child spawn.
#[cfg(any(unix, test))]
pub(crate) fn spawning() -> std::sync::RwLockReadGuard<'static, ()> {
    SPAWNING.read().expect("spawn lock is not poisoned")
}

/// The spawn lock held exclusively, so no other child spawns until it drops.
pub struct WatchdogSpawnGuard {
    _guard: RwLockWriteGuard<'static, ()>,
}

#[cfg(not(target_family = "wasm"))]
impl WatchdogSpawnGuard {
    pub fn spawn(&self, command: &mut Command) -> std::io::Result<Child> {
        command.spawn_unlocked()
    }
}

/// Wait for in-flight child spawns to finish and block new ones until the
/// guard drops. Only the process-group watchdog's spawn takes it.
pub fn watchdog_spawning() -> WatchdogSpawnGuard {
    WatchdogSpawnGuard { _guard: SPAWNING.write().expect("spawn lock is not poisoned") }
}

#[cfg(test)]
mod tests;

#[cfg(target_family = "wasm")]
pub type Command = command::Command<command::Blocking>;

#[cfg(not(target_family = "wasm"))]
mod native_command;

#[cfg(target_family = "wasm")]
mod child;
#[cfg(target_family = "wasm")]
mod command;
#[cfg(target_family = "wasm")]
mod host;
#[cfg(target_family = "wasm")]
mod pipe;
#[cfg(target_family = "wasm")]
mod status;

#[cfg(target_family = "wasm")]
mod signals;
