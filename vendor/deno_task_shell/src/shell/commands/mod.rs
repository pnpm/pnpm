// Copyright 2018-2025 the Deno authors. MIT license.

mod args;
mod cat;
mod cd;
mod cp_mv;
mod echo;
mod error;
mod executable;
mod exit;
mod export;
mod head;
mod mkdir;
mod pwd;
mod rm;
mod set;
mod shopt;
mod sleep;
mod test;
mod unset;
mod xargs;

use std::{
    collections::HashMap,
    ffi::OsString,
    rc::Rc,
};

use futures::future::LocalBoxFuture;

pub use executable::ExecutableCommand;

use super::types::{
    ExecuteResult,
    FutureExecuteResult,
    ShellPipeReader,
    ShellPipeWriter,
    ShellState,
};

pub fn builtin_commands() -> HashMap<String, Rc<dyn ShellCommand>> {
    HashMap::from([
        ("cat".to_string(), Rc::new(cat::CatCommand) as Rc<dyn ShellCommand>),
        ("cd".to_string(), Rc::new(cd::CdCommand) as Rc<dyn ShellCommand>),
        ("cp".to_string(), Rc::new(cp_mv::CpCommand) as Rc<dyn ShellCommand>),
        ("echo".to_string(), Rc::new(echo::EchoCommand) as Rc<dyn ShellCommand>),
        ("exit".to_string(), Rc::new(exit::ExitCommand) as Rc<dyn ShellCommand>),
        ("export".to_string(), Rc::new(export::ExportCommand) as Rc<dyn ShellCommand>),
        ("head".to_string(), Rc::new(head::HeadCommand) as Rc<dyn ShellCommand>),
        ("mkdir".to_string(), Rc::new(mkdir::MkdirCommand) as Rc<dyn ShellCommand>),
        ("mv".to_string(), Rc::new(cp_mv::MvCommand) as Rc<dyn ShellCommand>),
        ("pwd".to_string(), Rc::new(pwd::PwdCommand) as Rc<dyn ShellCommand>),
        ("rm".to_string(), Rc::new(rm::RmCommand) as Rc<dyn ShellCommand>),
        ("set".to_string(), Rc::new(set::SetCommand) as Rc<dyn ShellCommand>),
        ("shopt".to_string(), Rc::new(shopt::ShoptCommand) as Rc<dyn ShellCommand>),
        ("sleep".to_string(), Rc::new(sleep::SleepCommand) as Rc<dyn ShellCommand>),
        ("test".to_string(), Rc::new(test::TestCommand::new_test()) as Rc<dyn ShellCommand>),
        ("true".to_string(), Rc::new(ExitCodeCommand(0)) as Rc<dyn ShellCommand>),
        ("false".to_string(), Rc::new(ExitCodeCommand(1)) as Rc<dyn ShellCommand>),
        // POSIX null command: discards its arguments and always exits 0.
        // Commonly seen as `"test": ":"` in package.json no-op scripts.
        (":".to_string(), Rc::new(ExitCodeCommand(0)) as Rc<dyn ShellCommand>),
        ("unset".to_string(), Rc::new(unset::UnsetCommand) as Rc<dyn ShellCommand>),
        ("xargs".to_string(), Rc::new(xargs::XargsCommand) as Rc<dyn ShellCommand>),
    ])
}

pub struct ExecuteCommandArgsContext {
    pub args: Vec<OsString>,
    pub state: ShellState,
    pub stdin: ShellPipeReader,
    pub stdout: ShellPipeWriter,
    pub stderr: ShellPipeWriter,
}

pub struct ShellCommandContext {
    pub args: Vec<OsString>,
    pub state: ShellState,
    pub stdin: ShellPipeReader,
    pub stdout: ShellPipeWriter,
    pub stderr: ShellPipeWriter,
    pub execute_command_args: Box<dyn FnOnce(ExecuteCommandArgsContext) -> FutureExecuteResult>,
}

pub trait ShellCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult>;
}

macro_rules! execute_with_cancellation {
    ($result_expr:expr, $kill_signal:expr) => {
        tokio::select! {
          result = $result_expr => {
            result
          },
          signal = $kill_signal.wait_aborted() => {
            ExecuteResult::from_exit_code(signal.aborted_code())
          }
        }
    };
}

pub(super) use execute_with_cancellation;

struct ExitCodeCommand(i32);

impl ShellCommand for ExitCodeCommand {
    fn execute(&self, _context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        // ignores additional arguments
        Box::pin(futures::future::ready(ExecuteResult::from_exit_code(self.0)))
    }
}
