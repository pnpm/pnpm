// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{
        OsStr,
        OsString,
    },
    path::{
        Path,
        PathBuf,
    },
    rc::Rc,
    string::FromUtf8Error,
};

use futures::{
    FutureExt,
    future,
    future::LocalBoxFuture,
};
use thiserror::Error;
use tokio::task::JoinHandle;

use crate::{
    parser::{
        Command,
        CommandInner,
        IoFile,
        PipeSequence,
        PipeSequenceOperator,
        Pipeline,
        PipelineInner,
        Redirect,
        RedirectFd,
        RedirectOp,
        RedirectOpInput,
        RedirectOpOutput,
        Sequence,
        SequentialList,
        SimpleCommand,
        Word,
        WordPart,
    },
    shell::{
        commands::{
            ShellCommand,
            ShellCommandContext,
        },
        types::{
            EnvChange,
            ExecuteResult,
            FutureExecuteResult,
            KillSignal,
            ShellOptions,
            ShellPipeReader,
            ShellPipeWriter,
            ShellState,
            SignalKind,
            pipe,
        },
    },
};

use super::{
    command::{
        UnresolvedCommandName,
        execute_unresolved_command_name,
    },
    types::TreeExitCodeCell,
};

/// Executes a `SequentialList` of commands in a deno_task_shell environment.
///
/// This function accepts a list of commands, a map of environment variables, the current working directory,
/// and a map of custom shell commands. It sets up the shell state and then calls `execute_with_pipes`
/// with the standard input, output, and error streams.
///
/// # Arguments
/// * `list` - A `SequentialList` of commands to execute.
/// * `env_vars` - A map of environment variables which are set in the shell.
/// * `cwd` - The current working directory.
/// * `custom_commands` - A map of custom shell commands and there ShellCommand implementation.
/// * `kill_signal` - Use to send signals to spawned executables.
///
/// # Returns
/// The exit code of the command execution.
pub async fn execute(
    list: SequentialList,
    env_vars: HashMap<OsString, OsString>,
    cwd: PathBuf,
    custom_commands: HashMap<String, Rc<dyn ShellCommand>>,
    kill_signal: KillSignal,
) -> i32 {
    let state = ShellState::new(env_vars, cwd, custom_commands, kill_signal);
    execute_with_pipes(
        list,
        state,
        ShellPipeReader::stdin(),
        ShellPipeWriter::stdout(),
        ShellPipeWriter::stderr(),
    )
    .await
}

/// Executes a `SequentialList` of commands with specified input and output pipes.
///
/// This function accepts a list of commands, a shell state, and pipes for standard input, output, and error.
/// This function allows the user to retrive the data outputted by the execution and act on it using code.
/// This is made public for the use-case of running tests with shell execution in application depending on the library.
///
/// # Arguments
///
/// * `list` - A `SequentialList` of commands to execute.
/// * `state` - The current state of the shell, including environment variables and the current directory.
/// * `stdin` - A reader for the standard input stream.
/// * `stdout` - A writer for the standard output stream.
/// * `stderr` - A writer for the standard error stream.
///
/// # Returns
///
/// The exit code of the command execution.
pub async fn execute_with_pipes(
    list: SequentialList,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> i32 {
    // spawn a sequential list and pipe its output to the environment
    let result =
        execute_sequential_list(list, state, stdin, stdout, stderr, AsyncCommandBehavior::Wait)
            .await;

    match result {
        ExecuteResult::Exit(code, _) => code,
        ExecuteResult::Continue(exit_code, _, _) => exit_code,
    }
}

#[derive(Debug, PartialEq)]
enum AsyncCommandBehavior {
    Wait,
    Yield,
}

fn execute_sequential_list(
    list: SequentialList,
    mut state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
    async_command_behavior: AsyncCommandBehavior,
) -> FutureExecuteResult {
    async move {
        let mut final_exit_code = 0;
        let mut final_changes = Vec::new();
        let mut async_handles = Vec::new();
        let mut was_exit = false;
        for item in list.items {
            if item.is_async {
                let state = state.clone();
                let stdin = stdin.clone();
                let stdout = stdout.clone();
                let stderr = stderr.clone();
                async_handles.push(tokio::task::spawn_local(async move {
                    let main_signal = state.kill_signal().clone();
                    let tree_exit_code_cell = state.tree_exit_code_cell().clone();
                    let result =
                        execute_sequence(item.sequence, state, stdin, stdout, stderr).await;
                    let (exit_code, handles) = result.into_exit_code_and_handles();
                    wait_handles(exit_code, handles, &main_signal, &tree_exit_code_cell).await
                }));
            } else {
                let result = execute_sequence(
                    item.sequence,
                    state.clone(),
                    stdin.clone(),
                    stdout.clone(),
                    stderr.clone(),
                )
                .await;
                match result {
                    ExecuteResult::Exit(exit_code, handles) => {
                        async_handles.extend(handles);
                        final_exit_code = exit_code;
                        was_exit = true;
                        break;
                    }
                    ExecuteResult::Continue(exit_code, changes, handles) => {
                        state.apply_changes(&changes);
                        state.apply_env_var(OsStr::new("?"), OsStr::new(&exit_code.to_string()));
                        final_changes.extend(changes);
                        async_handles.extend(handles);
                        // use the final sequential item's exit code
                        final_exit_code = exit_code;
                        if exit_code != 0
                            && state
                                .shell_options()
                                .contains(crate::shell::types::ShellOptions::ERREXIT)
                        {
                            break;
                        }
                    }
                }
            }
        }

        // wait for async commands to complete
        if async_command_behavior == AsyncCommandBehavior::Wait {
            final_exit_code = wait_handles(
                final_exit_code,
                std::mem::take(&mut async_handles),
                state.kill_signal(),
                state.tree_exit_code_cell(),
            )
            .await;
        }

        if was_exit {
            ExecuteResult::Exit(final_exit_code, async_handles)
        } else {
            ExecuteResult::Continue(final_exit_code, final_changes, async_handles)
        }
    }
    .boxed_local()
}

async fn wait_handles(
    mut exit_code: i32,
    mut handles: Vec<JoinHandle<i32>>,
    kill_signal: &KillSignal,
    tree_exit_code_cell: &TreeExitCodeCell,
) -> i32 {
    if exit_code != 0 {
        // this section failed, so set it as the exit code
        tree_exit_code_cell.try_set(exit_code);
        kill_signal.send(SignalKind::SIGTERM);
    }
    // prefer surfacing the tree exit code because it's the main reason for the failure
    exit_code = tree_exit_code_cell.get().unwrap_or(exit_code);
    while !handles.is_empty() {
        let (result, _, remaining) = futures::future::select_all(handles).await;

        // prefer the first non-zero exit code
        let new_exit_code = result.unwrap();
        if exit_code == 0 && new_exit_code != 0 {
            exit_code = new_exit_code;
        }

        handles = remaining;
    }
    exit_code
}

fn execute_sequence(
    sequence: Sequence,
    mut state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    mut stderr: ShellPipeWriter,
) -> FutureExecuteResult {
    // requires boxed async because of recursive async
    async move {
        match sequence {
            Sequence::ShellVar(var) => ExecuteResult::Continue(
                0,
                vec![EnvChange::SetShellVar(
                    var.name.into(),
                    match evaluate_word(var.value, &state, stdin, stderr.clone()).await {
                        Ok(value) => value,
                        Err(err) => {
                            return err.into_exit_code(&mut stderr);
                        }
                    },
                )],
                Vec::new(),
            ),
            Sequence::BooleanList(list) => {
                let mut changes = vec![];
                let first_result = execute_sequence(
                    list.current,
                    state.clone(),
                    stdin.clone(),
                    stdout.clone(),
                    stderr.clone(),
                )
                .await;
                let (exit_code, mut async_handles) = match first_result {
                    ExecuteResult::Exit(_, _) => return first_result,
                    ExecuteResult::Continue(exit_code, sub_changes, async_handles) => {
                        state.apply_env_var(OsStr::new("?"), OsStr::new(&exit_code.to_string()));
                        state.apply_changes(&sub_changes);
                        changes.extend(sub_changes);
                        (exit_code, async_handles)
                    }
                };

                let next = if list.op.moves_next_for_exit_code(exit_code) {
                    Some(list.next)
                } else {
                    let mut next = list.next;
                    loop {
                        // boolean lists always move right on the tree
                        match next {
                            Sequence::BooleanList(list) => {
                                if list.op.moves_next_for_exit_code(exit_code) {
                                    break Some(list.next);
                                }
                                next = list.next;
                            }
                            _ => break None,
                        }
                    }
                };
                if let Some(next) = next {
                    let next_result = execute_sequence(next, state, stdin, stdout, stderr).await;
                    match next_result {
                        ExecuteResult::Exit(code, sub_handles) => {
                            async_handles.extend(sub_handles);
                            ExecuteResult::Exit(code, async_handles)
                        }
                        ExecuteResult::Continue(exit_code, sub_changes, sub_handles) => {
                            changes.extend(sub_changes);
                            async_handles.extend(sub_handles);
                            ExecuteResult::Continue(exit_code, changes, async_handles)
                        }
                    }
                } else {
                    ExecuteResult::Continue(exit_code, changes, async_handles)
                }
            }
            Sequence::Pipeline(pipeline) => {
                execute_pipeline(pipeline, state, stdin, stdout, stderr).await
            }
        }
    }
    .boxed_local()
}

async fn execute_pipeline(
    pipeline: Pipeline,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> ExecuteResult {
    let result = execute_pipeline_inner(pipeline.inner, state, stdin, stdout, stderr).await;
    if pipeline.negated {
        match result {
            ExecuteResult::Exit(code, handles) => ExecuteResult::Exit(code, handles),
            ExecuteResult::Continue(code, changes, handles) => {
                let new_code = if code == 0 { 1 } else { 0 };
                ExecuteResult::Continue(new_code, changes, handles)
            }
        }
    } else {
        result
    }
}

async fn execute_pipeline_inner(
    pipeline: PipelineInner,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> ExecuteResult {
    match pipeline {
        PipelineInner::Command(command) => {
            execute_command(command, state, stdin, stdout, stderr).await
        }
        PipelineInner::PipeSequence(pipe_sequence) => {
            execute_pipe_sequence(*pipe_sequence, state, stdin, stdout, stderr).await
        }
    }
}

#[derive(Debug)]
enum RedirectPipe {
    Input(ShellPipeReader),
    Output(ShellPipeWriter),
}

async fn resolve_redirect_pipe(
    redirect: &Redirect,
    state: &ShellState,
    stdin: &ShellPipeReader,
    stdout: &ShellPipeWriter,
    stderr: &mut ShellPipeWriter,
) -> Result<RedirectPipe, ExecuteResult> {
    match redirect.io_file.clone() {
        IoFile::Word(word) => {
            resolve_redirect_word_pipe(word, &redirect.op, state, stdin, stderr).await
        }
        IoFile::Fd(fd) => match &redirect.op {
            RedirectOp::Input(RedirectOpInput::Redirect) => {
                let _ = stderr.write_line(
                    "deno_task_shell: input redirecting file descriptors is not implemented",
                );
                Err(ExecuteResult::from_exit_code(1))
            }
            RedirectOp::Output(_op) => match fd {
                1 => Ok(RedirectPipe::Output(stdout.clone())),
                2 => Ok(RedirectPipe::Output(stderr.clone())),
                _ => {
                    let _ = stderr.write_line(
            "deno_task_shell: output redirecting file descriptors beyond stdout and stderr is not implemented",
          );
                    Err(ExecuteResult::from_exit_code(1))
                }
            },
        },
    }
}

async fn resolve_redirect_word_pipe(
    word: Word,
    redirect_op: &RedirectOp,
    state: &ShellState,
    stdin: &ShellPipeReader,
    stderr: &mut ShellPipeWriter,
) -> Result<RedirectPipe, ExecuteResult> {
    fn handle_std_result(
        output_path: &Path,
        std_file_result: std::io::Result<std::fs::File>,
        stderr: &mut ShellPipeWriter,
    ) -> Result<std::fs::File, ExecuteResult> {
        match std_file_result {
            Ok(std_file) => Ok(std_file),
            Err(err) => {
                let _ = stderr.write_line(&format!(
                    "error opening file for redirect ({}). {:#}",
                    output_path.display(),
                    err
                ));
                Err(ExecuteResult::from_exit_code(1))
            }
        }
    }

    let words = evaluate_word_parts(word.into_parts(), state, stdin.clone(), stderr.clone()).await;
    let words = match words {
        Ok(word) => word,
        Err(err) => {
            return Err(err.into_exit_code(stderr));
        }
    };
    // edge case that's not supported
    if words.is_empty() {
        let _ = stderr.write_line("redirect path must be 1 argument, but found 0");
        return Err(ExecuteResult::from_exit_code(1));
    } else if words.len() > 1 {
        let _ = stderr.write_line(&format!(
            concat!(
                "redirect path must be 1 argument, but found {0} ({1}). ",
                "Did you mean to quote it (ex. \"{1}\")?"
            ),
            words.len(),
            os_string_join(&words, " ").to_string_lossy()
        ));
        return Err(ExecuteResult::from_exit_code(1));
    }
    let output_path = &words[0];

    match &redirect_op {
        RedirectOp::Input(RedirectOpInput::Redirect) => {
            let output_path = state.cwd().join(output_path);
            let std_file_result = std::fs::OpenOptions::new().read(true).open(&output_path);
            handle_std_result(&output_path, std_file_result, stderr)
                .map(|std_file| RedirectPipe::Input(ShellPipeReader::from_std(std_file)))
        }
        RedirectOp::Output(op) => {
            // cross platform suppress output
            if output_path == "/dev/null" {
                return Ok(RedirectPipe::Output(ShellPipeWriter::null()));
            }
            let output_path = state.cwd().join(output_path);
            let is_append = *op == RedirectOpOutput::Append;
            let std_file_result = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .append(is_append)
                .truncate(!is_append)
                .open(&output_path);
            handle_std_result(&output_path, std_file_result, stderr)
                .map(|std_file| RedirectPipe::Output(ShellPipeWriter::from_std(std_file)))
        }
    }
}

async fn execute_command(
    command: Command,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    mut stderr: ShellPipeWriter,
) -> ExecuteResult {
    let (stdin, stdout, stderr) = if let Some(redirect) = &command.redirect {
        let pipe = match resolve_redirect_pipe(redirect, &state, &stdin, &stdout, &mut stderr).await
        {
            Ok(value) => value,
            Err(value) => return value,
        };
        match pipe {
            RedirectPipe::Input(pipe) => match redirect.maybe_fd {
                Some(_) => {
                    let _ = stderr.write_line(
                        "input redirects with file descriptors are not supported",
                    );
                    return ExecuteResult::from_exit_code(1);
                }
                None => (pipe, stdout, stderr),
            },
            RedirectPipe::Output(pipe) => match redirect.maybe_fd {
                Some(RedirectFd::Fd(2)) => (stdin, stdout, pipe),
                Some(RedirectFd::Fd(1)) | None => (stdin, pipe, stderr),
                Some(RedirectFd::Fd(_)) => {
                    let _ = stderr.write_line(
                        "only redirecting to stdout (1) and stderr (2) is supported",
                    );
                    return ExecuteResult::from_exit_code(1);
                }
                Some(RedirectFd::StdoutStderr) => (stdin, pipe.clone(), pipe),
            },
        }
    } else {
        (stdin, stdout, stderr)
    };
    match command.inner {
        CommandInner::Simple(command) => {
            execute_simple_command(command, state, stdin, stdout, stderr).await
        }
        CommandInner::Subshell(list) => execute_subshell(list, state, stdin, stdout, stderr).await,
    }
}

async fn execute_pipe_sequence(
    pipe_sequence: PipeSequence,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> ExecuteResult {
    let mut wait_tasks = vec![];
    let mut last_output = Some(stdin);
    let mut next_inner: Option<PipelineInner> = Some(pipe_sequence.into());
    while let Some(sequence) = next_inner.take() {
        let (output_reader, output_writer) = pipe();
        let (stderr, command) = match sequence {
            PipelineInner::PipeSequence(pipe_sequence) => {
                next_inner = Some(pipe_sequence.next);
                (
                    match pipe_sequence.op {
                        PipeSequenceOperator::Stdout => stderr.clone(),
                        PipeSequenceOperator::StdoutStderr => output_writer.clone(),
                    },
                    pipe_sequence.current,
                )
            }
            PipelineInner::Command(command) => (stderr.clone(), command),
        };
        wait_tasks.push(execute_command(
            command,
            state.clone(),
            last_output.take().unwrap(),
            output_writer.clone(),
            stderr.clone(),
        ));
        last_output = Some(output_reader);
    }
    let output_handle = tokio::task::spawn_blocking(|| {
        last_output
            .unwrap()
            .pipe_to_sender(stdout)
            .unwrap();
    });
    let results = futures::future::join_all(wait_tasks).await;
    output_handle.await.unwrap();

    // Determine exit code based on pipefail option
    let exit_code = if state.shell_options().contains(ShellOptions::PIPEFAIL) {
        // With pipefail: return the rightmost non-zero exit code, or 0 if all succeeded
        results
            .iter()
            .rev()
            .find_map(|r| {
                let code = match r {
                    ExecuteResult::Exit(c, _) => *c,
                    ExecuteResult::Continue(c, _, _) => *c,
                };
                if code != 0 { Some(code) } else { None }
            })
            .unwrap_or(0)
    } else {
        // Without pipefail: return the last command's exit code
        match results.last().unwrap() {
            ExecuteResult::Exit(code, _) => *code,
            ExecuteResult::Continue(code, _, _) => *code,
        }
    };

    let all_handles = results.into_iter().flat_map(|r| r.into_handles());
    ExecuteResult::Continue(exit_code, Vec::new(), all_handles.collect())
}

async fn execute_subshell(
    list: Box<SequentialList>,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> ExecuteResult {
    let result = execute_sequential_list(
        *list,
        state,
        stdin,
        stdout,
        stderr,
        // yield async commands to the parent
        AsyncCommandBehavior::Yield,
    )
    .await;

    match result {
        ExecuteResult::Exit(code, handles) => {
            // sub shells do not cause an exit
            ExecuteResult::Continue(code, Vec::new(), handles)
        }
        ExecuteResult::Continue(code, _env_changes, handles) => {
            // env changes are not propagated
            ExecuteResult::Continue(code, Vec::new(), handles)
        }
    }
}

async fn execute_simple_command(
    command: SimpleCommand,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    mut stderr: ShellPipeWriter,
) -> ExecuteResult {
    let args = evaluate_args(command.args, &state, stdin.clone(), stderr.clone()).await;
    let args = match args {
        Ok(args) => args,
        Err(err) => {
            return err.into_exit_code(&mut stderr);
        }
    };
    let mut state = state.clone();
    for env_var in command.env_vars {
        let value = evaluate_word(env_var.value, &state, stdin.clone(), stderr.clone()).await;
        let value = match value {
            Ok(value) => value,
            Err(err) => {
                return err.into_exit_code(&mut stderr);
            }
        };
        state.apply_env_var(OsStr::new(&env_var.name), OsStr::new(&value));
    }
    execute_command_args(args, state, stdin, stdout, stderr).await
}

fn execute_command_args(
    mut args: Vec<OsString>,
    state: ShellState,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    mut stderr: ShellPipeWriter,
) -> FutureExecuteResult {
    let command_name = if args.is_empty() { OsString::new() } else { args.remove(0) };
    if let Some(exit_code) = state.kill_signal().aborted_code() {
        Box::pin(future::ready(ExecuteResult::from_exit_code(exit_code)))
    } else if let Some(stripped_name) = command_name.to_string_lossy().strip_prefix('!') {
        let _ = stderr.write_line(
        &format!(concat!(
          "History expansion is not supported:\n",
          "  {}\n",
          "  ~\n\n",
          "Perhaps you meant to add a space after the exclamation point to negate the command?\n",
          "  ! {}",
        ), command_name.to_string_lossy(), stripped_name)
      );
        Box::pin(future::ready(ExecuteResult::from_exit_code(1)))
    } else {
        let command_context = ShellCommandContext {
            args,
            state,
            stdin,
            stdout,
            stderr,
            execute_command_args: Box::new(move |context| {
                execute_command_args(
                    context.args,
                    context.state,
                    context.stdin,
                    context.stdout,
                    context.stderr,
                )
            }),
        };
        match command_context.state.resolve_custom_command(&command_name) {
            Some(command) => command.execute(command_context),
            None => execute_unresolved_command_name(
                UnresolvedCommandName {
                    name: command_name,
                    base_dir: command_context.state.cwd().to_path_buf(),
                },
                command_context,
            ),
        }
    }
}

pub async fn evaluate_args(
    args: Vec<Word>,
    state: &ShellState,
    stdin: ShellPipeReader,
    stderr: ShellPipeWriter,
) -> Result<Vec<OsString>, EvaluateWordTextError> {
    let mut result = Vec::new();
    for arg in args {
        let parts =
            evaluate_word_parts(arg.into_parts(), state, stdin.clone(), stderr.clone()).await?;
        result.extend(parts);
    }
    Ok(result)
}

async fn evaluate_word(
    word: Word,
    state: &ShellState,
    stdin: ShellPipeReader,
    stderr: ShellPipeWriter,
) -> Result<OsString, EvaluateWordTextError> {
    let word_parts = evaluate_word_parts(word.into_parts(), state, stdin, stderr).await?;
    Ok(os_string_join(&word_parts, " "))
}

#[derive(Debug, Error)]
pub enum EvaluateWordTextError {
    #[error("glob: no matches found '{}'. {}", pattern, err)]
    InvalidPattern { pattern: String, err: glob::PatternError },
    #[error("glob: no matches found '{}'. Pattern part was not valid utf-8", part.to_string_lossy())]
    NotUtf8Pattern { part: OsString },
    #[error(
        "glob: no matches found '{}' (run `shopt -u failglob` to pass unmatched glob patterns literally)",
        pattern
    )]
    NoFilesMatched { pattern: String },
    #[error("invalid utf-8: {}", err)]
    InvalidUtf8 {
        #[from]
        err: FromUtf8Error,
    },
    #[error("failed resolving home directory for tilde expansion")]
    NoHomeDirectory,
}

impl EvaluateWordTextError {
    pub fn into_exit_code(self, stderr: &mut ShellPipeWriter) -> ExecuteResult {
        let _ = stderr.write_line(&self.to_string());
        ExecuteResult::from_exit_code(1)
    }
}

fn evaluate_word_parts(
    parts: Vec<WordPart>,
    state: &ShellState,
    stdin: ShellPipeReader,
    stderr: ShellPipeWriter,
) -> LocalBoxFuture<'_, Result<Vec<OsString>, EvaluateWordTextError>> {
    #[derive(Debug)]
    enum TextPart {
        Quoted(OsString),
        Text(OsString),
    }

    impl TextPart {
        pub fn as_str(&self) -> &OsStr {
            match self {
                TextPart::Quoted(text) => text,
                TextPart::Text(text) => text,
            }
        }
    }

    fn text_parts_to_string(parts: Vec<TextPart>) -> OsString {
        let mut result = OsString::with_capacity(
            parts
                .iter()
                .map(|p| p.as_str().len())
                .sum(),
        );
        for part in parts {
            result.push(part.as_str());
        }
        result
    }

    fn evaluate_word_text(
        state: &ShellState,
        text_parts: Vec<TextPart>,
        is_quoted: bool,
    ) -> Result<Vec<OsString>, EvaluateWordTextError> {
        if !is_quoted
            && text_parts
                .iter()
                .filter_map(|p| match p {
                    TextPart::Quoted(_) => None,
                    TextPart::Text(text) => text.to_str(),
                })
                .any(|text| {
                    text.chars()
                        .any(|c| matches!(c, '?' | '*' | '['))
                })
        {
            let mut current_text = String::new();
            for text_part in text_parts {
                match text_part {
                    TextPart::Quoted(text) => {
                        if let Some(text) = text.to_str() {
                            for c in text.chars() {
                                match c {
                                    '?' | '*' | '[' | ']' => {
                                        // escape because it was quoted
                                        current_text.push('[');
                                        current_text.push(c);
                                        current_text.push(']');
                                    }
                                    _ => current_text.push(c),
                                }
                            }
                        } else {
                            return Err(EvaluateWordTextError::NotUtf8Pattern { part: text });
                        }
                    }
                    TextPart::Text(text) => {
                        if let Some(text) = text.to_str() {
                            current_text.push_str(text);
                        } else {
                            return Err(EvaluateWordTextError::NotUtf8Pattern { part: text });
                        }
                    }
                }
            }
            let is_absolute = Path::new(&current_text).is_absolute();
            let cwd = state.cwd();
            let options = state.shell_options();

            // when globstar is disabled, replace ** with * so it doesn't match
            // across directory boundaries (the glob crate always treats ** as recursive)
            let pattern_text: Cow<str> =
                if !options.contains(ShellOptions::GLOBSTAR) && current_text.contains("**") {
                    // repeatedly replace ** with * to handle cases like *** -> ** -> *
                    let mut result = current_text.clone();
                    while result.contains("**") {
                        result = result.replace("**", "*");
                    }
                    Cow::Owned(result)
                } else {
                    Cow::Borrowed(&current_text)
                };

            let pattern = if is_absolute {
                pattern_text.into_owned()
            } else {
                format!("{}/{}", cwd.display(), pattern_text)
            };

            let result = glob::glob_with(
                &pattern,
                glob::MatchOptions {
                    // false because it should work the same way on case insensitive file systems
                    case_sensitive: false,
                    // true because it copies what sh does
                    require_literal_separator: true,
                    // true because it copies with sh does—these files are considered "hidden"
                    require_literal_leading_dot: true,
                },
            );
            match result {
                Ok(paths) => {
                    let paths = paths
                        .into_iter()
                        .filter_map(|p| p.ok())
                        .collect::<Vec<_>>();
                    if paths.is_empty() {
                        // failglob - error when set
                        if options.contains(ShellOptions::FAILGLOB) {
                            Err(EvaluateWordTextError::NoFilesMatched { pattern })
                        } else if options.contains(ShellOptions::NULLGLOB) {
                            // nullglob - return empty vec (pattern expands to nothing)
                            Ok(Vec::new())
                        } else {
                            // default bash behavior - return pattern literally
                            Ok(vec![current_text.into()])
                        }
                    } else {
                        let paths = if is_absolute {
                            paths
                                .into_iter()
                                .map(|p| p.into_os_string())
                                .collect::<Vec<_>>()
                        } else {
                            paths
                                .into_iter()
                                .map(|p| {
                                    let path = p.strip_prefix(cwd).unwrap();
                                    path.to_path_buf().into_os_string()
                                })
                                .collect::<Vec<_>>()
                        };
                        Ok(paths)
                    }
                }
                Err(err) => {
                    // bash passes malformed patterns through literally; only error when
                    // failglob is explicitly set
                    if options.contains(ShellOptions::FAILGLOB) {
                        Err(EvaluateWordTextError::InvalidPattern { pattern, err })
                    } else if options.contains(ShellOptions::NULLGLOB) {
                        Ok(Vec::new())
                    } else {
                        Ok(vec![current_text.into()])
                    }
                }
            }
        } else {
            Ok(vec![text_parts_to_string(text_parts)])
        }
    }

    fn evaluate_word_parts_inner(
        parts: Vec<WordPart>,
        is_quoted: bool,
        state: &ShellState,
        stdin: ShellPipeReader,
        stderr: ShellPipeWriter,
    ) -> LocalBoxFuture<'_, Result<Vec<OsString>, EvaluateWordTextError>> {
        // recursive async, so requires boxing
        async move {
            let mut result = Vec::new();
            let mut current_text = Vec::new();
            let mut pending: std::collections::VecDeque<_> = parts.into();
            while let Some(part) = pending.pop_front() {
                let evaluation_result_text = match part {
                    WordPart::Text(text) => {
                        current_text.push(TextPart::Text(text.into()));
                        None
                    }
                    WordPart::Variable(name) => state
                        .get_var(OsStr::new(&name))
                        .cloned(),
                    WordPart::ParameterExpansion {
                        name,
                        empty_is_unset,
                        alternate,
                        word,
                    } => {
                        let has_value = state
                            .get_var(OsStr::new(&name))
                            .is_some_and(|value| !empty_is_unset || !value.is_empty());
                        if has_value == alternate {
                            for part in word.into_iter().rev() {
                                pending.push_front(part);
                            }
                            continue;
                        } else if alternate {
                            None
                        } else {
                            state
                                .get_var(OsStr::new(&name))
                                .cloned()
                        }
                    }
                    WordPart::FieldBoundary => {
                        if !current_text.is_empty() {
                            result.extend(evaluate_word_text(
                                state,
                                std::mem::take(&mut current_text),
                                is_quoted,
                            )?);
                        }
                        continue;
                    }
                    WordPart::Tilde => Some(
                        sys_traits::impls::real_home_dir_with_env(state)
                            .map(|s| s.into_os_string())
                            .ok_or(EvaluateWordTextError::NoHomeDirectory)?,
                    ),
                    WordPart::Command(list) => Some(
                        evaluate_command_substitution(
                            list,
                            // contain cancellation to the command substitution
                            &state.with_child_signal(),
                            stdin.clone(),
                            stderr.clone(),
                        )
                        .await?,
                    ),
                    WordPart::Quoted(parts) => {
                        let parts = evaluate_word_parts_inner(
                            parts,
                            true,
                            state,
                            stdin.clone(),
                            stderr.clone(),
                        )
                        .await?;
                        let text = os_string_join(&parts, " ");

                        current_text.push(TextPart::Quoted(text));
                        continue;
                    }
                    // Brace alternatives are pre-expanded by `expand_braces` before
                    // we ever reach the inner evaluator.
                    WordPart::Brace(_) => {
                        unreachable!("brace parts must be expanded before evaluation")
                    }
                };

                if let Some(text) = evaluation_result_text {
                    // turn the text into a vector of strings
                    let mut parts = split_osstring_on_space(&text)
                        .into_iter()
                        .map(TextPart::Text)
                        .collect::<Vec<_>>();

                    if !parts.is_empty() {
                        // append the first part to the current text
                        let first_part = parts.remove(0);
                        current_text.push(first_part);

                        if !parts.is_empty() {
                            // evaluate and store the current text
                            result.extend(evaluate_word_text(state, current_text, is_quoted)?);

                            // store all the parts except the last one
                            for part in parts.drain(..parts.len() - 1) {
                                result.extend(evaluate_word_text(state, vec![part], is_quoted)?);
                            }

                            // use the last part as the current text so it maybe
                            // gets appended to in the future
                            current_text = parts;
                        }
                    }
                }
            }
            if !current_text.is_empty() {
                result.extend(evaluate_word_text(state, current_text, is_quoted)?);
            }
            Ok(result)
        }
        .boxed_local()
    }

    async move {
        let mut all_results = Vec::new();
        for expanded in expand_braces(parts) {
            let parts_result =
                evaluate_word_parts_inner(expanded, false, state, stdin.clone(), stderr.clone())
                    .await?;
            all_results.extend(parts_result);
        }
        Ok(all_results)
    }
    .boxed_local()
}

/// Expand a flat list of [`WordPart`]s containing any number of
/// [`WordPart::Brace`] alternatives into a list of brace-free part lists,
/// one per cartesian-product combination of the alternatives.
fn expand_braces(parts: Vec<WordPart>) -> Vec<Vec<WordPart>> {
    let mut results: Vec<Vec<WordPart>> = vec![Vec::new()];
    for part in parts {
        match part {
            WordPart::Brace(alternatives) => {
                let expanded: Vec<Vec<WordPart>> = alternatives
                    .into_iter()
                    .flat_map(|word| expand_braces(word.into_parts()))
                    .collect();
                if expanded.is_empty() {
                    continue;
                }
                let mut next = Vec::with_capacity(results.len() * expanded.len());
                for prefix in &results {
                    for alt in &expanded {
                        let mut combined = Vec::with_capacity(prefix.len() + alt.len());
                        combined.extend(prefix.iter().cloned());
                        combined.extend(alt.iter().cloned());
                        next.push(combined);
                    }
                }
                results = next;
            }
            other => {
                for r in &mut results {
                    r.push(other.clone());
                }
            }
        }
    }
    results
}

async fn evaluate_command_substitution(
    list: SequentialList,
    state: &ShellState,
    stdin: ShellPipeReader,
    stderr: ShellPipeWriter,
) -> Result<OsString, FromUtf8Error> {
    let data = execute_with_stdout(|shell_stdout_writer| {
        execute_sequential_list(
            list,
            state.clone(),
            stdin,
            shell_stdout_writer,
            stderr,
            AsyncCommandBehavior::Wait,
        )
    })
    .await;

    let data = trim_and_normalize_whitespaces_to_spaces(&data);
    os_string_from_bytes(data)
}

// Remove the trailing newline and then replace inner whitespace with a space
// This seems to be what sh does, but I'm not entirely sure:
//
// > echo $(echo 1 && echo -e "\n\t2\n\t\n\n")
// 1 2
fn trim_and_normalize_whitespaces_to_spaces(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut iter = input.iter().copied().peekable();
    let mut in_word = false;

    // skip leading whitespace
    while let Some(b) = iter.peek() {
        if matches!(b, b' ' | b'\n' | b'\r' | b'\t') {
            iter.next();
        } else {
            break;
        }
    }

    while let Some(b) = iter.next() {
        if matches!(b, b' ' | b'\n' | b'\r' | b'\t') {
            // skip all subsequent whitespace
            while let Some(n) = iter.peek() {
                if matches!(n, b' ' | b'\n' | b'\r' | b'\t') {
                    iter.next();
                } else {
                    break;
                }
            }

            // only emit a space if we've already emitted some content
            if in_word && iter.peek().is_some() {
                output.push(b' ');
            }
        } else {
            output.push(b);
            in_word = true;
        }
    }

    output
}

async fn execute_with_stdout(
    execute: impl FnOnce(ShellPipeWriter) -> FutureExecuteResult,
) -> Vec<u8> {
    let (shell_stdout_reader, shell_stdout_writer) = pipe();
    let spawned_output = execute(shell_stdout_writer);
    let output_handle = tokio::task::spawn_blocking(move || {
        let mut final_data = Vec::new();
        shell_stdout_reader.pipe_to(&mut final_data).unwrap();
        final_data
    });
    let _ = spawned_output.await;
    output_handle.await.unwrap()
}

#[cfg(unix)]
fn split_osstring_on_space(text: &OsStr) -> Vec<OsString> {
    use std::os::unix::ffi::OsStrExt;

    // todo(dsherret): how to not use from_utf8_lossy here?
    let text = String::from_utf8_lossy(text.as_bytes());
    text.split(' ')
        .filter(|s| !s.is_empty())
        .map(OsString::from)
        .collect::<Vec<_>>()
}

#[cfg(windows)]
fn split_osstring_on_space(text: &OsStr) -> Vec<OsString> {
    use std::os::windows::ffi::{
        OsStrExt,
        OsStringExt,
    };
    let wide: Vec<u16> = text.encode_wide().collect();

    wide.split(|&w| w == 0x20) // split on spaces
        .filter(|chunk| !chunk.is_empty()) // remove empty pieces from multiple spaces
        .map(OsString::from_wide)
        .collect()
}

#[cfg(unix)]
fn os_string_from_bytes(bytes: Vec<u8>) -> Result<OsString, FromUtf8Error> {
    use std::os::unix::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_vec(bytes))
}

#[cfg(windows)]
fn os_string_from_bytes(bytes: Vec<u8>) -> Result<OsString, FromUtf8Error> {
    String::from_utf8(bytes).map(OsString::from)
}

fn os_string_join(parts: &[OsString], join_text: &str) -> OsString {
    if parts.is_empty() {
        return OsString::new();
    }
    let capacity = parts
        .iter()
        .map(|p| p.len())
        .sum::<usize>()
        + (parts.len() - 1) * join_text.len();
    let mut result = OsString::with_capacity(capacity);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            result.push(join_text);
        }
        result.push(part);
    }
    debug_assert_eq!(result.len(), capacity);
    result
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_trim_and_normalize_whitespaces_to_spaces() {
        fn get_output(input: &str) -> String {
            String::from_utf8(trim_and_normalize_whitespaces_to_spaces(input.as_bytes())).unwrap()
        }

        assert_eq!(
            get_output("   \t  \r   \n testing this \t \r\n \n \n    out\n \t \r\n"),
            "testing this out"
        )
    }
}
