use super::{
    READ_BUFFER_SIZE, extract_env_document, extract_main_document, read_first_yaml_document,
    read_first_yaml_document_in_chunks,
};
use std::io::{self, Read};

#[test]
fn returns_entire_content_when_it_does_not_start_with_separator() {
    let content = "lockfileVersion: 9.0\npackages: {}\n";
    assert_eq!(extract_main_document(content), content);
}

#[test]
fn returns_empty_string_when_content_starts_with_separator_but_has_no_second_separator() {
    let content = "---\nfoo: bar\n";
    assert_eq!(extract_main_document(content), "");
}

#[test]
fn returns_the_second_document_from_a_combined_file() {
    let main = "lockfileVersion: 9.0\npackages: {}\n";
    let combined = format!("---\nfoo: bar\n---\n{main}");
    assert_eq!(extract_main_document(&combined), main);
}

#[test]
fn splits_a_crlf_combined_file() {
    let combined = "---\r\nfoo: bar\r\n---\r\nlockfileVersion: 9.0\r\npackages: {}\r\n";
    assert_eq!(extract_main_document(combined), "lockfileVersion: 9.0\npackages: {}\n");
    assert_eq!(extract_env_document(combined).as_deref(), Some("foo: bar"));
}

#[test]
fn splits_a_combined_file_behind_a_byte_order_mark() {
    let combined = "\u{feff}---\nfoo: bar\n---\nlockfileVersion: 9.0\n";
    assert_eq!(extract_main_document(combined), "lockfileVersion: 9.0\n");
    assert_eq!(extract_env_document(combined).as_deref(), Some("foo: bar"));
}

#[test]
fn normalizes_a_single_document_file() {
    let content = "\u{feff}lockfileVersion: 9.0\r\npackages: {}\r\n";
    assert_eq!(extract_main_document(content), "lockfileVersion: 9.0\npackages: {}\n");
    assert_eq!(extract_env_document(content), None);
}

/// Every marker the streaming reader looks for — the byte-order mark,
/// the document start, a CRLF pair, the separator — is longer than one
/// byte, so each chunk size puts a different one across a boundary.
#[track_caller]
fn assert_streams(content: &str, expected: Option<&str>) {
    for chunk_size in [1, 2, 3, 4, 5, 6, 7, 64, READ_BUFFER_SIZE] {
        let read = read_first_yaml_document_in_chunks(content.as_bytes(), chunk_size)
            .expect("read the env document");
        assert_documents_eq(read.as_deref(), expected, &format!("chunk size {chunk_size}"));
    }
    let read = read_first_yaml_document(content.as_bytes()).expect("read the env document");
    let extracted = extract_env_document(content);
    assert_documents_eq(read.as_deref(), expected, "default chunk size");
    assert_documents_eq(read.as_deref(), extracted.as_deref(), "against extract_env_document");
}

/// Logs both documents with `{}` before failing: `assert_eq!` renders a
/// multiline document as one escaped line, and the fixtures here run to
/// six figures of bytes, so logging every comparison would bury the one
/// that failed.
#[track_caller]
fn assert_documents_eq(read: Option<&str>, expected: Option<&str>, context: &str) {
    if read != expected {
        eprintln!(
            "{context}\nREAD:\n{}\n\nEXPECTED:\n{}\n",
            read.unwrap_or("<none>"),
            expected.unwrap_or("<none>"),
        );
    }
    assert_eq!(read, expected, "{context}");
}

#[test]
fn streams_the_env_document_of_a_combined_file() {
    assert_streams("---\nfoo: bar\n---\nlockfileVersion: 9.0\n", Some("foo: bar"));
}

#[test]
fn streams_a_multiline_env_document() {
    let env = "lockfileVersion: env-1.0\nimporters:\n  .:\n    foo: bar";
    assert_streams(&format!("---\n{env}\n---\nlockfileVersion: 9.0\n"), Some(env));
}

#[test]
fn streams_an_empty_env_document() {
    assert_streams("---\n\n---\nlockfileVersion: 9.0\n", Some(""));
}

#[test]
fn streams_no_env_document_when_the_file_does_not_start_with_a_marker() {
    assert_streams("lockfileVersion: 9.0\npackages: {}\n", None);
}

#[test]
fn streams_no_env_document_when_the_separator_is_missing() {
    assert_streams("---\nfoo: bar\n", None);
}

#[test]
fn streams_no_env_document_from_an_empty_file() {
    assert_streams("", None);
}

#[test]
fn streams_an_env_document_behind_a_byte_order_mark() {
    assert_streams("\u{feff}---\nfoo: bar\n---\nlockfileVersion: 9.0\n", Some("foo: bar"));
}

#[test]
fn streams_a_crlf_env_document() {
    assert_streams("---\r\nfoo: bar\r\n---\r\nlockfileVersion: 9.0\r\n", Some("foo: bar"));
}

#[test]
fn streams_a_crlf_env_document_behind_a_byte_order_mark() {
    assert_streams("\u{feff}---\r\nfoo: bar\r\n---\r\nlockfileVersion: 9.0\r\n", Some("foo: bar"));
}

#[test]
fn keeps_a_lone_carriage_return_verbatim() {
    assert_streams("---\nfoo: b\rar\n---\nlockfileVersion: 9.0\n", Some("foo: b\rar"));
}

#[test]
fn streams_an_env_document_longer_than_one_chunk() {
    let env = format!("packages:\n{}", "  foo@1.0.0: {}\n".repeat(10_000));
    let env = env.trim_end_matches('\n');
    assert_streams(&format!("---\n{env}\n---\nlockfileVersion: 9.0\n"), Some(env));
}

#[test]
fn stops_reading_at_the_separator() {
    let combined = format!("---\nfoo: bar\n---\n{}", "packages: {}\n".repeat(100_000));
    let mut reader = CountingReader::new(&combined);

    let env = read_first_yaml_document(&mut reader).expect("read the env document");

    assert_eq!(env.as_deref(), Some("foo: bar"));
    assert_eq!(reader.read, READ_BUFFER_SIZE, "the main document must stay unread");
}

#[test]
fn stops_reading_a_lockfile_without_an_env_document_after_one_chunk() {
    let main = "packages: {}\n".repeat(100_000);
    let mut reader = CountingReader::new(&main);

    let env = read_first_yaml_document(&mut reader).expect("read the env document");

    assert_eq!(env, None);
    assert_eq!(reader.read, READ_BUFFER_SIZE);
}

#[test]
fn retries_a_signal_interrupted_read() {
    let content = "---\nfoo: bar\n---\nlockfileVersion: 9.0\n";
    let reader = InterruptingReader { content: content.as_bytes(), interrupt_next: true };

    let env = read_first_yaml_document_in_chunks(reader, 4).expect("read the env document");

    assert_eq!(env.as_deref(), Some("foo: bar"));
}

#[test]
fn reports_an_env_document_that_is_not_utf8() {
    let mut content = b"---\nfoo: ".to_vec();
    content.extend_from_slice(&[0xff, 0xfe]);
    content.extend_from_slice(b"\n---\nlockfileVersion: 9.0\n");

    let error = read_first_yaml_document(content.as_slice()).expect_err("invalid UTF-8 must fail");

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

/// Interrupts every other read, as a signal arriving mid-read does.
struct InterruptingReader<'a> {
    content: &'a [u8],
    interrupt_next: bool,
}

impl Read for InterruptingReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.interrupt_next {
            self.interrupt_next = false;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        self.interrupt_next = true;
        self.content.read(buf)
    }
}

struct CountingReader<'a> {
    content: &'a [u8],
    read: usize,
}

impl<'a> CountingReader<'a> {
    fn new(content: &'a str) -> Self {
        CountingReader { content: content.as_bytes(), read: 0 }
    }
}

impl Read for CountingReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.content.read(buf)?;
        self.read += read;
        Ok(read)
    }
}
