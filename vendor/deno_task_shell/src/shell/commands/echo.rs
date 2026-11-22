// Copyright 2018-2025 the Deno authors. MIT license.

use futures::future::LocalBoxFuture;

use crate::shell::types::ExecuteResult;

use super::{
    ShellCommand,
    ShellCommandContext,
};

pub struct EchoCommand;

impl ShellCommand for EchoCommand {
    fn execute(&self, mut context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let iter = context.args
            .iter()
            .enumerate()
            .flat_map(|(i, arg)| -> Box<dyn Iterator<Item = &[u8]>> {
                if i == 0 {
                    Box::new(std::iter::once(arg.as_encoded_bytes()))
                } else {
                    Box::new(
                        std::iter::once(" ".as_bytes())
                            .chain(std::iter::once(arg.as_encoded_bytes())),
                    )
                }
            })
            .chain(Box::new(std::iter::once("\n".as_bytes())));
        _ = context.stdout.write_all_iter(iter);
        Box::pin(futures::future::ready(ExecuteResult::from_exit_code(0)))
    }
}
