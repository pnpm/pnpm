pub mod asynchronous {
    #[cfg(target_family = "wasm")]
    pub use crate::{ChildStderr, ChildStdin, ChildStdout, child::AsyncChild as Child};
    #[cfg(not(target_family = "wasm"))]
    pub use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
    #[cfg(target_family = "wasm")]
    pub type Command = crate::command::Command<crate::command::Async>;
}

#[cfg(target_family = "wasm")]
pub use child::{Child, Output};
#[cfg(target_family = "wasm")]
pub use command::Stdio;
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
    Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio, id,
};
pub use std::process::{ExitCode, abort, exit};

#[cfg(target_family = "wasm")]
pub type Command = command::Command<command::Blocking>;

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
