// Copyright 2018-2025 the Deno authors. MIT license.

use futures::future::LocalBoxFuture;

use crate::shell::types::{
    EnvChange,
    ExecuteResult,
};

use super::{
    ShellCommand,
    ShellCommandContext,
};

pub struct ExportCommand;

impl ShellCommand for ExportCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let mut changes = Vec::new();
        for arg in context.args {
            // todo: support non-UTF8 data here
            if let Some(arg) = arg.to_str() {
                // ignore if it doesn't contain an equals
                if let Some(equals_index) = arg.find('=') {
                    let arg_name = &arg[..equals_index];
                    let arg_value = &arg[equals_index + 1..];
                    changes.push(EnvChange::SetEnvVar(arg_name.into(), arg_value.into()));
                }
            }
        }
        let result = ExecuteResult::Continue(0, changes, Vec::new());
        Box::pin(futures::future::ready(result))
    }
}
