use super::{LoggerLevel, next_line_bounded, parse_logger_line};
use tokio::io::BufReader;

#[tokio::test]
async fn next_line_bounded_truncates_long_lines_and_keeps_draining() {
    let long = "x".repeat(100);
    let input = format!("{long}\nafter\n");
    let mut reader = BufReader::new(input.as_bytes());
    assert_eq!(next_line_bounded(&mut reader, 10).await.unwrap().unwrap(), "x".repeat(10));
    assert_eq!(next_line_bounded(&mut reader, 10).await.unwrap().unwrap(), "after");
    assert_eq!(next_line_bounded(&mut reader, 10).await.unwrap(), None);
}

#[tokio::test]
async fn next_line_bounded_survives_invalid_utf8() {
    let mut reader = BufReader::new(&b"bad\xffbyte\nnext\n"[..]);
    assert_eq!(next_line_bounded(&mut reader, 64).await.unwrap().unwrap(), "bad\u{fffd}byte");
    assert_eq!(next_line_bounded(&mut reader, 64).await.unwrap().unwrap(), "next");
}

#[tokio::test]
async fn next_line_bounded_returns_final_unterminated_line() {
    let mut reader = BufReader::new(&b"no newline"[..]);
    assert_eq!(next_line_bounded(&mut reader, 64).await.unwrap().unwrap(), "no newline");
    assert_eq!(next_line_bounded(&mut reader, 64).await.unwrap(), None);
}

#[test]
fn parse_logger_line_accepts_only_the_wrapper_protocol() {
    let (level, message) = parse_logger_line(r#"{"level":"info","message":"hi"}"#).unwrap();
    assert!(matches!(level, LoggerLevel::Info));
    assert_eq!(message, "hi");

    let (level, message) = parse_logger_line(r#"{"level":"warn","message":42}"#).unwrap();
    assert!(matches!(level, LoggerLevel::Warn));
    assert_eq!(message, "42");

    // Hook-printed JSON and plain text fall through to raw forwarding.
    assert!(parse_logger_line(r#"{"foo":"bar"}"#).is_none());
    assert!(parse_logger_line(r#"{"level":"debug","message":"hi"}"#).is_none());
    assert!(parse_logger_line(r#"{"level":"info"}"#).is_none());
    assert!(parse_logger_line("plain text").is_none());
}
