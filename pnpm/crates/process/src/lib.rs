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

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static SPAWNING: RwLock<()> = RwLock::new(());

pub fn spawning() -> RwLockReadGuard<'static, ()> {
    SPAWNING.read().expect("spawn lock is not poisoned")
}

pub struct WatchdogSpawnGuard {
    _guard: RwLockWriteGuard<'static, ()>,
}

#[cfg(not(target_family = "wasm"))]
impl WatchdogSpawnGuard {
    pub fn spawn(&self, command: &mut Command) -> std::io::Result<Child> {
        command.spawn_unlocked()
    }
}

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
