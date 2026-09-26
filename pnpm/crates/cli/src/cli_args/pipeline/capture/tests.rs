use super::{Buffer, MAX_CAPTURE_BYTES};
use pnpm_reporter::LifecycleStdio;

#[test]
fn oversized_logs_release_capture_and_keep_it_disabled() {
    let mut buffer = Buffer::default();
    buffer.push(LifecycleStdio::Stdout, "hello");
    buffer.push(LifecycleStdio::Stdout, &"x".repeat(MAX_CAPTURE_BYTES));
    buffer.push(LifecycleStdio::Stdout, "after limit");
    assert!(buffer.exceeded, "oversized stage must bypass the cache");
    assert!(buffer.lines.is_empty(), "oversized stage must not retain its output");
}
