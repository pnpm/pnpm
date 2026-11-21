// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    ffi::{
        OsStr,
        OsString,
    },
    fs::File,
    io::Read,
};

use futures::future::LocalBoxFuture;

use crate::{
    ExecuteResult,
    ShellCommand,
    ShellCommandContext,
    ShellPipeWriter,
    shell::KillSignal,
};

use super::{
    args::{
        ArgKind,
        parse_arg_kinds,
    },
    error::{
        ShellCommandError,
        bail,
    },
};

pub struct HeadCommand;

impl ShellCommand for HeadCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        let mut stderr = context.stderr.clone();
        let result = match execute_head(context) {
            Ok(result) => result,
            Err(err) => {
                let _ = stderr.write_line(&format!("head: {err}"));
                ExecuteResult::from_exit_code(1)
            }
        };
        Box::pin(futures::future::ready(result))
    }
}

fn copy_lines<F: FnMut(&mut [u8]) -> std::io::Result<usize>>(
    writer: &mut ShellPipeWriter,
    max_lines: u64,
    kill_signal: &KillSignal,
    mut read: F,
    buffer_size: usize,
) -> std::io::Result<ExecuteResult> {
    let mut written_lines = 0;
    let mut buffer = vec![0; buffer_size];
    while written_lines < max_lines {
        if let Some(exit_code) = kill_signal.aborted_code() {
            return Ok(ExecuteResult::from_exit_code(exit_code));
        }
        let read_bytes = read(&mut buffer)?;
        if read_bytes == 0 {
            break;
        }

        if let Some(exit_code) = kill_signal.aborted_code() {
            return Ok(ExecuteResult::from_exit_code(exit_code));
        }

        let mut written_bytes: usize = 0;
        let split_lines = buffer[..read_bytes].split(|&b| b == b'\n');
        for line in split_lines {
            if written_lines >= max_lines || (written_bytes + line.len()) >= read_bytes {
                break;
            }
            writer.write_all(line)?;
            writer.write_all(b"\n")?;
            written_bytes += line.len() + 1;
            written_lines += 1;
        }

        if written_lines < max_lines && written_bytes < read_bytes {
            writer.write_all(&buffer[written_bytes..read_bytes])?;
        }
    }

    Ok(ExecuteResult::from_exit_code(0))
}

fn copy_all_but_last_lines<F: FnMut(&mut [u8]) -> std::io::Result<usize>>(
    writer: &mut ShellPipeWriter,
    skip_last: u64,
    kill_signal: &KillSignal,
    mut read: F,
) -> std::io::Result<ExecuteResult> {
    // read all content first
    let mut content = Vec::new();
    let mut buffer = vec![0; 512];
    loop {
        if let Some(exit_code) = kill_signal.aborted_code() {
            return Ok(ExecuteResult::from_exit_code(exit_code));
        }
        let read_bytes = read(&mut buffer)?;
        if read_bytes == 0 {
            break;
        }
        content.extend_from_slice(&buffer[..read_bytes]);
    }

    // count total lines
    let total_lines = content
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u64;

    // output all but the last N lines
    if total_lines <= skip_last {
        return Ok(ExecuteResult::from_exit_code(0));
    }
    let lines_to_print = total_lines - skip_last;

    let mut line_count = 0u64;
    let mut start = 0;
    for (i, &b) in content.iter().enumerate() {
        if let Some(exit_code) = kill_signal.aborted_code() {
            return Ok(ExecuteResult::from_exit_code(exit_code));
        }
        if b == b'\n' {
            line_count += 1;
            if line_count <= lines_to_print {
                writer.write_all(&content[start..=i])?;
            }
            start = i + 1;
            if line_count >= lines_to_print {
                break;
            }
        }
    }

    Ok(ExecuteResult::from_exit_code(0))
}

fn execute_head(mut context: ShellCommandContext) -> Result<ExecuteResult, ShellCommandError> {
    let flags = parse_args(&context.args)?;
    match flags.lines {
        LineCount::First(max_lines) => {
            if flags.path == "-" {
                Ok(copy_lines(
                    &mut context.stdout,
                    max_lines,
                    context.state.kill_signal(),
                    |buf| context.stdin.read(buf),
                    512,
                )?)
            } else {
                let path = flags.path;
                match File::open(context.state.cwd().join(path)) {
                    Ok(mut file) => Ok(copy_lines(
                        &mut context.stdout,
                        max_lines,
                        context.state.kill_signal(),
                        |buf| file.read(buf),
                        512,
                    )?),
                    Err(err) => {
                        context.stderr.write_line(&format!(
                            "head: {}: {}",
                            path.to_string_lossy(),
                            err
                        ))?;
                        Ok(ExecuteResult::from_exit_code(1))
                    }
                }
            }
        }
        LineCount::AllButLast(skip_last) => {
            if flags.path == "-" {
                Ok(copy_all_but_last_lines(
                    &mut context.stdout,
                    skip_last,
                    context.state.kill_signal(),
                    |buf| context.stdin.read(buf),
                )?)
            } else {
                let path = flags.path;
                match File::open(context.state.cwd().join(path)) {
                    Ok(mut file) => Ok(copy_all_but_last_lines(
                        &mut context.stdout,
                        skip_last,
                        context.state.kill_signal(),
                        |buf| file.read(buf),
                    )?),
                    Err(err) => {
                        context.stderr.write_line(&format!(
                            "head: {}: {}",
                            path.to_string_lossy(),
                            err
                        ))?;
                        Ok(ExecuteResult::from_exit_code(1))
                    }
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum LineCount {
    /// print first N lines
    First(u64),
    /// print all but last N lines
    AllButLast(u64),
}

#[derive(Debug, PartialEq)]
struct HeadFlags<'a> {
    path: &'a OsStr,
    lines: LineCount,
}

fn parse_line_count(s: &str) -> Result<LineCount, ShellCommandError> {
    if let Some(rest) = s.strip_prefix('-') {
        let num = rest.parse::<u64>()?;
        Ok(LineCount::AllButLast(num))
    } else {
        let num = s.parse::<u64>()?;
        Ok(LineCount::First(num))
    }
}

fn parse_args<'a>(args: &'a [OsString]) -> Result<HeadFlags<'a>, ShellCommandError> {
    let mut path: Option<&'a OsStr> = None;
    let mut lines: Option<LineCount> = None;
    let mut iterator = parse_arg_kinds(args).into_iter();
    while let Some(arg) = iterator.next() {
        match arg {
            ArgKind::Arg(file_name) => {
                if path.is_none() {
                    path = Some(file_name);
                    continue;
                }

                // for now, we only support one file
                // TODO: support multiple files
                bail!("only one file is supported for now");
            }
            ArgKind::ShortFlag('n') => match iterator.next() {
                Some(ArgKind::Arg(arg)) => {
                    if let Some(s) = arg.to_str() {
                        match parse_line_count(s) {
                            Ok(count) => lines = Some(count),
                            Err(_) => bail!("expected a numeric value following -n"),
                        }
                    } else {
                        bail!("expected a numeric value following -n")
                    }
                }
                _ => bail!("expected a value following -n"),
            },
            ArgKind::LongFlag(flag) => {
                if flag == "lines" || flag == "lines=" {
                    bail!("expected a value for --lines");
                } else if let Some(arg) = flag.strip_prefix("lines=") {
                    lines = Some(parse_line_count(arg)?);
                } else {
                    arg.bail_unsupported()?
                }
            }
            _ => arg.bail_unsupported()?,
        }
    }

    Ok(HeadFlags {
        path: path.unwrap_or(OsStr::new("-")),
        lines: lines.unwrap_or(LineCount::First(10)),
    })
}

#[cfg(test)]
mod test {
    use crate::pipe;
    use std::cmp::min;

    use super::*;
    use pretty_assertions::assert_eq;

    async fn copies_lines(
        // #[case]
        buffer_size: usize,
    ) {
        let (reader, mut writer) = pipe();
        let reader_handle = reader.pipe_to_string_handle();
        let data = b"foo\nbar\nbaz\nqux\n";
        let data_length = data.len();
        let mut offset = 0;
        let result = copy_lines(
            &mut writer,
            2,
            &KillSignal::default(),
            |buffer| {
                if offset >= data.len() {
                    return Ok(0);
                }
                let buffer_length = buffer.len();
                let read_length = min(buffer_length, data_length);
                buffer[..read_length].copy_from_slice(&data[offset..(offset + read_length)]);
                offset += read_length;
                Ok(read_length)
            },
            buffer_size,
        );
        drop(writer); // Drop the writer ahead of the reader to prevent a deadlock.
        assert_eq!(reader_handle.await.unwrap(), "foo\nbar\n");
        assert_eq!(result.unwrap().into_exit_code_and_handles().0, 0);
    }

    #[tokio::test]
    async fn copies_lines_with_shorter_buffer_size() {
        copies_lines(2).await;
    }

    #[tokio::test]
    async fn copies_lines_with_buffer_size_to_match_each_line_length() {
        copies_lines(4).await;
    }

    #[tokio::test]
    async fn copies_lines_with_buffer_of_one_and_half_times_of_each_line_length() {
        copies_lines(6).await;
    }

    #[tokio::test]
    async fn copies_lines_with_long_buffer_size() {
        copies_lines(512).await;
    }

    #[test]
    fn parses_args() {
        assert_eq!(
            parse_args(&[]).unwrap(),
            HeadFlags { path: OsStr::new("-"), lines: LineCount::First(10) }
        );
        assert_eq!(
            parse_args(&["-n".into(), "5".into()]).unwrap(),
            HeadFlags { path: OsStr::new("-"), lines: LineCount::First(5) }
        );
        assert_eq!(
            parse_args(&["--lines=5".into()]).unwrap(),
            HeadFlags { path: OsStr::new("-"), lines: LineCount::First(5) }
        );
        assert_eq!(
            parse_args(&["path".into()]).unwrap(),
            HeadFlags { path: OsStr::new("path"), lines: LineCount::First(10) }
        );
        assert_eq!(
            parse_args(&["-n".into(), "5".into(), "path".into()]).unwrap(),
            HeadFlags { path: OsStr::new("path"), lines: LineCount::First(5) }
        );
        assert_eq!(
            parse_args(&["--lines=5".into(), "path".into()]).unwrap(),
            HeadFlags { path: OsStr::new("path"), lines: LineCount::First(5) }
        );
        assert_eq!(
            parse_args(&["path".into(), "-n".into(), "5".into()]).unwrap(),
            HeadFlags { path: OsStr::new("path"), lines: LineCount::First(5) }
        );
        assert_eq!(
            parse_args(&["path".into(), "--lines=5".into()]).unwrap(),
            HeadFlags { path: OsStr::new("path"), lines: LineCount::First(5) }
        );
        // negative line counts (all but last N)
        assert_eq!(
            parse_args(&["-n".into(), "-1".into()]).unwrap(),
            HeadFlags { path: OsStr::new("-"), lines: LineCount::AllButLast(1) }
        );
        assert_eq!(
            parse_args(&["-n".into(), "-5".into(), "path".into()]).unwrap(),
            HeadFlags { path: OsStr::new("path"), lines: LineCount::AllButLast(5) }
        );
        assert_eq!(
            parse_args(&["--lines=-3".into()]).unwrap(),
            HeadFlags { path: OsStr::new("-"), lines: LineCount::AllButLast(3) }
        );
        assert_eq!(
            parse_args(&["-n".into()])
                .err()
                .unwrap()
                .to_string(),
            "expected a value following -n"
        );
        assert_eq!(
            parse_args(&["--lines".into()])
                .err()
                .unwrap()
                .to_string(),
            "expected a value for --lines"
        );
        assert_eq!(
            parse_args(&["--lines=".into()])
                .err()
                .unwrap()
                .to_string(),
            "expected a value for --lines"
        );
        assert_eq!(
            parse_args(&["--flag".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: --flag"
        );
        assert_eq!(
            parse_args(&["-t".into()])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: -t"
        );
    }

    #[tokio::test]
    async fn copies_all_but_last_lines() {
        let (reader, mut writer) = pipe();
        let reader_handle = reader.pipe_to_string_handle();
        let data = b"line1\nline2\nline3\nline4\nline5\n";
        let mut offset = 0;
        let result = copy_all_but_last_lines(&mut writer, 1, &KillSignal::default(), |buffer| {
            if offset >= data.len() {
                return Ok(0);
            }
            let read_length = min(buffer.len(), data.len() - offset);
            buffer[..read_length].copy_from_slice(&data[offset..(offset + read_length)]);
            offset += read_length;
            Ok(read_length)
        });
        drop(writer);
        assert_eq!(reader_handle.await.unwrap(), "line1\nline2\nline3\nline4\n");
        assert_eq!(result.unwrap().into_exit_code_and_handles().0, 0);
    }

    #[tokio::test]
    async fn copies_all_but_last_two_lines() {
        let (reader, mut writer) = pipe();
        let reader_handle = reader.pipe_to_string_handle();
        let data = b"line1\nline2\nline3\nline4\nline5\n";
        let mut offset = 0;
        let result = copy_all_but_last_lines(&mut writer, 2, &KillSignal::default(), |buffer| {
            if offset >= data.len() {
                return Ok(0);
            }
            let read_length = min(buffer.len(), data.len() - offset);
            buffer[..read_length].copy_from_slice(&data[offset..(offset + read_length)]);
            offset += read_length;
            Ok(read_length)
        });
        drop(writer);
        assert_eq!(reader_handle.await.unwrap(), "line1\nline2\nline3\n");
        assert_eq!(result.unwrap().into_exit_code_and_handles().0, 0);
    }

    #[tokio::test]
    async fn copies_all_but_last_lines_when_skip_exceeds_total() {
        let (reader, mut writer) = pipe();
        let reader_handle = reader.pipe_to_string_handle();
        let data = b"line1\nline2\n";
        let mut offset = 0;
        let result = copy_all_but_last_lines(&mut writer, 5, &KillSignal::default(), |buffer| {
            if offset >= data.len() {
                return Ok(0);
            }
            let read_length = min(buffer.len(), data.len() - offset);
            buffer[..read_length].copy_from_slice(&data[offset..(offset + read_length)]);
            offset += read_length;
            Ok(read_length)
        });
        drop(writer);
        // when skip_last >= total_lines, output should be empty
        assert_eq!(reader_handle.await.unwrap(), "");
        assert_eq!(result.unwrap().into_exit_code_and_handles().0, 0);
    }
}
