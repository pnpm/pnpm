//! A plain-HTTP server that answers every request alike, for tests of what
//! libsql sends.

use std::{
    io::{
        BufRead,
        BufReader,
        Write,
    },
    net::{
        TcpListener,
        TcpStream,
    },
    sync::{
        Arc,
        atomic::{
            AtomicUsize,
            Ordering,
        },
        mpsc::{
            self,
            Receiver,
        },
    },
    time::{
        Duration,
        Instant,
    },
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

/// A loopback plain-HTTP server that keeps its connections open while they
/// are in use.
pub struct ClosingIdleServer {
    pub url: String,
    /// The connections the server accepted.
    pub connections: Arc<AtomicUsize>,
}

/// Answer requests with `body`, keeping each connection open, and close a
/// connection unanswered when a request arrives after it idled `idle_limit`:
/// what a client sees when it reuses a connection just as the server closes
/// it for idling.
#[must_use]
pub fn serve_closing_idle_http(body: &'static str, idle_limit: Duration) -> ClosingIdleServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted = Arc::clone(&connections);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            accepted.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || answer_until_idle(stream, body, idle_limit));
        }
    });
    ClosingIdleServer { url, connections }
}

fn answer_until_idle(mut stream: TcpStream, body: &str, idle_limit: Duration) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len(),
    );
    let mut answered: Option<Instant> = None;
    while reader
        .fill_buf()
        .is_ok_and(|buffered| !buffered.is_empty())
    {
        if answered.is_some_and(|answered| answered.elapsed() >= idle_limit) {
            return;
        }
        read_request(&mut reader);
        if stream.write_all(response.as_bytes()).is_err() {
            return;
        }
        answered = Some(Instant::now());
    }
}

/// Answer one request with `body` and return the request's head.
fn answer(mut stream: TcpStream, body: &str) -> String {
    let mut reader = BufReader::new(&stream);
    let head = read_request(&mut reader);
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes()).unwrap();
    head
}

/// Read one request, its body included, and return its head.
fn read_request(reader: &mut impl BufRead) -> String {
    let head = read_head(reader);
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
    head
}

fn read_head(reader: &mut impl BufRead) -> String {
    let mut head = String::new();
    while reader.read_line(&mut head).unwrap() > 2 {}
    head
}
