use super::{AsyncBufRead, AsyncBufReadExt, AsyncRead, BufReader};
use tokio::io::AsyncReadExt as _;

/// How much trailing stderr to keep for the failure message when the hook
/// exits non-zero.
pub(super) const STDERR_TAIL_LIMIT: usize = 64 * 1024;

/// Longest hook stdout line kept; the remainder of an over-long line is
/// discarded so hook-controlled output cannot grow memory without bound.
const STDOUT_LINE_LIMIT: usize = 64 * 1024;

/// Forwards each of the one-shot hook's stdout lines to the Rust-side logger
/// closures as it arrives, which emit them as `pnpm:hook` events. Lines the
/// JS wrapper's logger writes carry their level; everything else the hook
/// prints (e.g. its own `console.log`) is forwarded as info so it is not
/// silently lost.
pub(super) async fn forward_hook_stdout(
    stdout: tokio::process::ChildStdout,
    logger: &crate::PreResolutionHookLogger,
) {
    let mut reader = BufReader::new(stdout);
    while let Ok(Some(line)) = next_line_bounded(&mut reader, STDOUT_LINE_LIMIT).await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_logger_line(line) {
            Some((LoggerLevel::Info, message)) => (logger.info)(message),
            Some((LoggerLevel::Warn, message)) => (logger.warn)(message),
            None => (logger.info)(line.to_string()),
        }
    }
}

pub(super) enum LoggerLevel {
    Info,
    Warn,
}

/// Parses one line of the JS wrapper's logger protocol,
/// `{"level":"info"|"warn","message":...}`. Anything else — non-JSON, or
/// JSON the hook printed itself — returns `None` so the caller forwards it
/// verbatim.
pub(super) fn parse_logger_line(line: &str) -> Option<(LoggerLevel, String)> {
    if !line.starts_with('{') {
        return None;
    }
    let parsed = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let level = match parsed.get("level").and_then(|v| v.as_str()) {
        Some("info") => LoggerLevel::Info,
        Some("warn") => LoggerLevel::Warn,
        _ => return None,
    };
    let message = match parsed.get("message")? {
        v if v.is_string() => v.as_str().unwrap().to_string(),
        v => v.to_string(),
    };
    Some((level, message))
}

/// Reads one `\n`-terminated line, keeping at most `cap` bytes of it and
/// discarding the rest, and decodes it lossily. Unlike
/// [`AsyncBufReadExt::read_line`] this neither buffers an unbounded line nor
/// stops on invalid UTF-8 — the pipe must keep draining either way, or the
/// child blocks on a full pipe until the hook timeout. Returns `None` at EOF.
pub(super) async fn next_line_bounded(
    reader: &mut (impl AsyncBufRead + Unpin),
    cap: usize,
) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok((!line.is_empty()).then(|| lossy_string(&line)));
        }
        let (consumed, line_complete) = take_buffered_line(available, cap, &mut line);
        reader.consume(consumed);
        if line_complete {
            return Ok(Some(lossy_string(&line)));
        }
    }
}

/// Take what one buffered read contributes to the line: the bytes up to the
/// cap, and how many to consume. The second value says whether a newline ended
/// the line.
fn take_buffered_line(available: &[u8], cap: usize, line: &mut Vec<u8>) -> (usize, bool) {
    let newline = available.iter().position(|&byte| byte == b'\n');
    let visible = newline.unwrap_or(available.len());
    let keep = visible.min(cap.saturating_sub(line.len()));
    line.extend_from_slice(&available[..keep]);
    match newline {
        Some(position) => (position + 1, true),
        None => (available.len(), false),
    }
}

fn lossy_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Reads `stream` to EOF keeping only the final `cap` bytes, so a
/// hook-controlled pipe cannot grow the buffer without bound.
pub(super) async fn read_tail(stream: impl AsyncRead + Unpin, cap: usize) -> Vec<u8> {
    let mut stream = stream;
    let mut tail = Vec::new();
    // Heap-allocated so the read buffer doesn't bloat the future
    // (`clippy::large_futures`).
    let mut buf = vec![0u8; 8192];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                // Trim only past twice the cap so noisy output memmoves the
                // tail once per `cap` bytes, not once per read.
                if tail.len() > cap * 2 {
                    tail.drain(..tail.len() - cap);
                }
            }
        }
    }
    tail.drain(..tail.len().saturating_sub(cap));
    tail
}
