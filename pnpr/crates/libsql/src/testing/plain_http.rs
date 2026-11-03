//! A plain-HTTP server that answers every request alike, for tests of what
//! libsql sends.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
};

/// The Hrana pipeline response to libsql's request for one statement:
/// a batch with an empty step result, the autocommit state, and the close.
pub const EXECUTE_RESPONSE: &str = r#"{"baton":null,"base_url":null,"results":[{"type":"ok","response":{"type":"batch","result":{"step_results":[{"cols":[],"rows":[],"affected_row_count":0,"last_insert_rowid":null}],"step_errors":[null]}}},{"type":"ok","response":{"type":"get_autocommit","is_autocommit":true}},{"type":"ok","response":{"type":"close"}}]}"#;

/// A loopback plain-HTTP server that answers every request alike.
pub struct PlainHttpServer {
    pub url: String,
    /// The head of every request the server received, in order.
    pub requests: Receiver<String>,
}

/// Answer every request on a loopback plain-HTTP server with `body`.
#[must_use]
pub fn serve_plain_http(body: &'static str) -> PlainHttpServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (sender, requests) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = sender.send(answer(stream, body));
        }
    });
    PlainHttpServer { url, requests }
}

/// Answer one request with `body` and return the request's head.
fn answer(mut stream: TcpStream, body: &str) -> String {
    let mut reader = BufReader::new(&stream);
    let head = read_head(&mut reader);
    let content_length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_owned)
        })
        .map(|value| value.trim().parse().unwrap())
        .unwrap_or(0);
    reader
        .read_exact(&mut vec![0; content_length])
        .unwrap();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes()).unwrap();
    head
}

fn read_head(reader: &mut impl BufRead) -> String {
    let mut head = String::new();
    while reader.read_line(&mut head).unwrap() > 2 {}
    head
}
