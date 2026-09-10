use super::{
    AsyncBufReadExt, AsyncBufReader, AsyncRead, BufRead, BufReader, Child, ExitStatus,
    LifecycleLog, LifecycleMessage, LifecycleStdio, LogEvent, LogLevel, Read, io, thread,
};

pub(super) const STREAMED_OUTPUT_CHUNK_BYTES: usize = 64 * 1024;

/// A script whose output is republished as `pnpm:lifecycle` events
/// rather than written straight to the terminal.
///
/// The three identity fields travel on every event the script produces:
/// `dep_path` groups them, `stage` names the script, and `wd` is what the
/// reporter renders as the project prefix.
#[derive(Clone, Copy)]
pub struct StreamedScript<'a> {
    pub dep_path: &'a str,
    pub stage: &'a str,
    pub wd: &'a str,
    pub emit: fn(&LogEvent),
}

impl StreamedScript<'_> {
    /// Announce the script that is about to run.
    pub fn started(&self, script: &str) {
        (self.emit)(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Script {
                dep_path: self.dep_path.to_string(),
                optional: false,
                script: script.to_string(),
                stage: self.stage.to_string(),
                wd: self.wd.to_string(),
            },
        }));
    }

    /// Announce how the script ended. `-1` stands for a child killed by a
    /// signal, which carries no exit code.
    pub fn finished(&self, exit_code: i32) {
        (self.emit)(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Exit {
                dep_path: self.dep_path.to_string(),
                exit_code,
                optional: false,
                stage: self.stage.to_string(),
                wd: self.wd.to_string(),
            },
        }));
    }

    /// Drain `child`'s piped stdout and stderr into one event per line,
    /// then wait for it. The pumps are joined after the wait, so every
    /// line is emitted before the caller's [`Self::finished`] — the
    /// ordering pnpm's reporter renders against.
    ///
    /// The child must have been spawned with both streams piped;
    /// whichever is absent is simply not pumped.
    pub fn pump(&self, child: &mut Child) -> io::Result<ExitStatus> {
        let stdout_handle =
            child.stdout.take().map(|stream| self.pump_stream(stream, LifecycleStdio::Stdout));
        let stderr_handle =
            child.stderr.take().map(|stream| self.pump_stream(stream, LifecycleStdio::Stderr));
        let status = child.wait();
        if let Some(handle) = stdout_handle {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }
        status
    }

    /// Asynchronously drain a tokio child's piped stdout and stderr into
    /// lifecycle events, then wait for it.
    pub async fn pump_async(&self, child: &mut tokio::process::Child) -> io::Result<ExitStatus> {
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_pump = async {
            if let Some(stream) = stdout {
                self.pump_async_stream(stream, LifecycleStdio::Stdout).await;
            }
        };
        let stderr_pump = async {
            if let Some(stream) = stderr {
                self.pump_async_stream(stream, LifecycleStdio::Stderr).await;
            }
        };
        let child_wait = child.wait();
        let (status, (), ()) = tokio::join!(child_wait, stdout_pump, stderr_pump);
        status
    }

    /// Spawn a thread that reads `reader` and republishes newline-delimited
    /// output in bounded chunks.
    ///
    /// Read as bytes and decoded lossily rather than through
    /// [`BufRead::lines`], whose `Err` on non-UTF-8 would stop the drain
    /// while the child is still writing — the child then blocks on a
    /// full pipe and the caller's `wait` never returns. pnpm decodes the
    /// same output lossily.
    pub(super) fn pump_stream(
        &self,
        reader: impl Read + Send + 'static,
        stdio: LifecycleStdio,
    ) -> thread::JoinHandle<()> {
        let (dep_path, stage, wd) =
            (self.dep_path.to_string(), self.stage.to_string(), self.wd.to_string());
        let emit = self.emit;
        thread::spawn(move || {
            let target = StreamedScript { dep_path: &dep_path, stage: &stage, wd: &wd, emit };
            pump_lines(&target, reader, stdio);
        })
    }

    async fn pump_async_stream(&self, reader: impl AsyncRead + Unpin, stdio: LifecycleStdio) {
        let mut reader = AsyncBufReader::new(reader);
        let mut line = Vec::new();
        while let Ok(buffered) = reader.fill_buf().await {
            if buffered.is_empty() {
                if !line.is_empty() {
                    self.emit_bytes_line(stdio, &mut line);
                }
                break;
            }
            let (consumed, line_finished) = take_streamed_chunk(buffered, &mut line);
            reader.consume(consumed);
            if line_finished {
                self.emit_bytes_line(stdio, &mut line);
                line.clear();
            }
        }
    }

    fn emit_bytes_line(&self, stdio: LifecycleStdio, line: &mut Vec<u8>) {
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        self.emit_line(stdio, String::from_utf8_lossy(line).into_owned());
    }

    /// Republish one line of the script's output.
    pub fn emit_line(&self, stdio: LifecycleStdio, line: String) {
        (self.emit)(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Stdio {
                dep_path: self.dep_path.to_string(),
                line,
                stage: self.stage.to_string(),
                stdio,
                wd: self.wd.to_string(),
            },
        }));
    }
}

/// Read one stream to EOF, emitting a log line per newline or per full chunk.
///
/// An `EBADF` or `EPIPE` means the child closed the stream. Not fatal — the
/// caller's `wait` surfaces a non-zero exit code if the child failed over it.
fn pump_lines(target: &StreamedScript<'_>, reader: impl Read, stdio: LifecycleStdio) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    while let Ok(buffered) = reader.fill_buf() {
        if buffered.is_empty() {
            if !line.is_empty() {
                target.emit_bytes_line(stdio, &mut line);
            }
            break;
        }
        let (consumed, line_finished) = take_streamed_chunk(buffered, &mut line);
        reader.consume(consumed);
        if line_finished {
            target.emit_bytes_line(stdio, &mut line);
            line.clear();
        }
    }
}

/// Take what one buffered read contributes to the line, reporting how many
/// bytes to consume and whether the line is ready to emit.
fn take_streamed_chunk(buffered: &[u8], line: &mut Vec<u8>) -> (usize, bool) {
    let consumed = streamed_chunk_len(buffered, line.len());
    line.extend_from_slice(&buffered[..consumed]);
    let finished = line.last() == Some(&b'\n') || line.len() == STREAMED_OUTPUT_CHUNK_BYTES;
    (consumed, finished)
}

fn streamed_chunk_len(buffered: &[u8], accumulated: usize) -> usize {
    let through_newline =
        buffered.iter().position(|byte| *byte == b'\n').map_or(buffered.len(), |i| i + 1);
    through_newline.min(STREAMED_OUTPUT_CHUNK_BYTES - accumulated)
}
