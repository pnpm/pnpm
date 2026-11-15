use std::time::Duration;

use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

use super::{
    super::{
        VerifyLockfileOptions,
        resolve::{NdjsonLimits, read_ndjson_frames},
    },
    PnprClient, PnprClientError, matching_applied_policies, matching_transform_lockfile,
    resolve_projects_options,
};

async fn drain_http_request(socket: &mut tokio::net::TcpStream) {
    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if socket.read(&mut byte).await.unwrap_or(0) == 0 {
            return;
        }
        buf.push(byte[0]);
    }
    let headers = String::from_utf8_lossy(&buf);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        let _ = socket.read_exact(&mut body).await;
    }
}

async fn wait_for_client_close(socket: &mut tokio::net::TcpStream) {
    let mut probe = [0u8; 1];
    let _ = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut probe)).await;
}

#[tokio::test]
async fn read_ndjson_frames_enforces_line_limit_across_single_and_split_chunks() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    let server = tokio::spawn(async move {
        // 1. Single chunk containing a newline-terminated line over the 32-byte cap.
        let (mut sock1, _) = listener.accept().await.unwrap();
        drain_http_request(&mut sock1).await;
        sock1
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut oversized = vec![b'a'; 40];
        oversized.push(b'\n');
        let _ = sock1.write_all(&oversized).await;
        let _ = sock1.flush().await;
        wait_for_client_close(&mut sock1).await;
        drop(sock1);

        // 2. Multi-chunk stream without a newline exceeding the 32-byte cap.
        let (mut sock2, _) = listener.accept().await.unwrap();
        drain_http_request(&mut sock2).await;
        sock2
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        sock2
            .write_all(&[b'x'; 20])
            .await
            .unwrap();
        sock2.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _ = sock2.write_all(&[b'y'; 20]).await;
        let _ = sock2.flush().await;
        wait_for_client_close(&mut sock2).await;
        drop(sock2);

        // 3. Multi-chunk valid frames within the 32-byte cap.
        let (mut sock3, _) = listener.accept().await.unwrap();
        drain_http_request(&mut sock3).await;
        sock3
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        sock3.write_all(b"{\"n\":1}\n{\"n\"").await.unwrap();
        sock3.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        sock3.write_all(b":2}\n").await.unwrap();
        let _ = sock3.shutdown().await;
    });

    let http = reqwest::Client::new();
    let limits = NdjsonLimits { max_line_bytes: 32, max_lines: 8 };

    let resp1 = http.get(&url).send().await.unwrap();
    let err1 = read_ndjson_frames(resp1, limits, |_| Ok(Some(()))).await.unwrap_err();
    assert!(
        matches!(err1, PnprClientError::Protocol(ref msg) if msg.contains("32-byte limit")),
        "expected line-limit error for single chunk, got {err1:?}",
    );

    let resp2 = http.get(&url).send().await.unwrap();
    let err2 = read_ndjson_frames(resp2, limits, |_| Ok(Some(()))).await.unwrap_err();
    assert!(
        matches!(err2, PnprClientError::Protocol(ref msg) if msg.contains("32-byte limit")),
        "expected line-limit error for split chunk without newline, got {err2:?}",
    );

    let resp3 = http.get(&url).send().await.unwrap();
    let mut seen = Vec::new();
    let done = read_ndjson_frames(resp3, limits, |line| {
        seen.push(String::from_utf8(line.to_vec()).unwrap());
        Ok((seen.len() == 2).then_some(42))
    })
    .await
    .unwrap();
    assert_eq!(done, Some(42));
    assert_eq!(seen, vec![r#"{"n":1}"#, r#"{"n":2}"#]);

    server.await.unwrap();
}

#[tokio::test]
async fn verify_lockfile_rejects_excessive_non_terminal_ndjson_lines() {
    let mut server = mockito::Server::new_async().await;
    let stream_body = "\n".repeat(70);
    let verify_mock = server
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(stream_body)
        .create_async()
        .await;

    let mut options = resolve_projects_options();
    options.reuse.lockfile =
        Some(serde_json::from_value(matching_transform_lockfile(&options)).unwrap());
    let verify_opts = VerifyLockfileOptions::from_resolve_projects_options(&options).unwrap();

    let err = PnprClient::new(server.url()).verify_lockfile(verify_opts).await.unwrap_err();
    assert!(
        matches!(err, PnprClientError::Protocol(ref msg) if msg.contains("64-line limit")),
        "expected line-count limit error, got {err:?}",
    );
    verify_mock.assert_async().await;
}

#[tokio::test]
async fn verify_lockfile_rejects_oversized_unterminated_ndjson_line() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_http_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let chunk = vec![b'a'; 64 * 1024];
        // Write 32 MiB + 64 KiB without `\n`, then hold the socket open until the
        // client aborts mid-stream upon crossing the 32 MiB frame cap.
        for _ in 0..513 {
            if socket.write_all(&chunk).await.is_err() {
                return;
            }
        }
        let _ = socket.flush().await;
        wait_for_client_close(&mut socket).await;
    });

    let mut options = resolve_projects_options();
    options.reuse.lockfile =
        Some(serde_json::from_value(matching_transform_lockfile(&options)).unwrap());
    let verify_opts = VerifyLockfileOptions::from_resolve_projects_options(&options).unwrap();

    let err = PnprClient::new(url).verify_lockfile(verify_opts).await.unwrap_err();
    assert!(
        matches!(err, PnprClientError::Protocol(ref msg) if msg.contains("NDJSON frame exceeds the 33554432-byte limit")),
        "unexpected error: {err:?}",
    );

    server.await.unwrap();
}

#[tokio::test]
async fn verify_lockfile_decodes_violations_frame_over_four_mib() {
    let mut server = mockito::Server::new_async().await;
    let large_reason = "v".repeat(4 * 1024 * 1024 + 128 * 1024);
    let mut frame = serde_json::to_vec(&json!({
        "type": "violations",
        "violations": [{
            "name": "acme",
            "version": "1.0.0",
            "code": "ERR_PNPM_VULNERABLE_PACKAGE",
            "reason": large_reason,
        }],
    }))
    .unwrap();
    frame.push(b'\n');

    let verify_mock = server
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(frame)
        .create_async()
        .await;

    let mut options = resolve_projects_options();
    options.reuse.lockfile =
        Some(serde_json::from_value(matching_transform_lockfile(&options)).unwrap());
    let verify_opts = VerifyLockfileOptions::from_resolve_projects_options(&options).unwrap();

    let err = PnprClient::new(server.url()).verify_lockfile(verify_opts).await.unwrap_err();
    let is_verification = matches!(err, PnprClientError::Verification(_));
    assert!(
        is_verification,
        "expected decoded Verification error rather than frame-limit failure, got {err:?}",
    );
    verify_mock.assert_async().await;
}

#[tokio::test]
async fn resolve_and_verify_bound_non_2xx_error_bodies() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "pnpr": {
                    "protocols": {
                        "resolver": { "versions": [0], "features": ["projectTransforms"] }
                    }
                }
            })
            .to_string(),
        )
        .expect_at_least(0)
        .create_async()
        .await;

    let huge_error = "e".repeat(65 * 1024);
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(502)
        .with_body(&huge_error)
        .create_async()
        .await;
    let verify_mock = server
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(502)
        .with_body(&huge_error)
        .create_async()
        .await;

    let mut options = resolve_projects_options();
    options.reuse.lockfile =
        Some(serde_json::from_value(matching_transform_lockfile(&options)).unwrap());
    let verify_opts = VerifyLockfileOptions::from_resolve_projects_options(&options).unwrap();

    let resolve_err = PnprClient::new(server.url()).resolve_projects(options).await.unwrap_err();
    assert!(
        matches!(resolve_err, PnprClientError::Server { ref message, code: None } if message.contains("65536-byte limit")),
        "unexpected resolve error: {resolve_err:?}",
    );

    let verify_err = PnprClient::new(server.url()).verify_lockfile(verify_opts).await.unwrap_err();
    assert!(
        matches!(verify_err, PnprClientError::Server { ref message, code: None } if message.contains("65536-byte limit")),
        "unexpected verify error: {verify_err:?}",
    );

    // Also verify structured JSON error code on a bounded 4xx response.
    let mut options2 = resolve_projects_options();
    let _ = matching_applied_policies(&options2);
    options2.transforms.patched_dependencies = None;
    options2.transforms.package_extensions = None;
    let coded_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"graph too large","code":"ERR_PNPR_GRAPH_TOO_LARGE"}"#)
        .create_async()
        .await;
    let coded_err = PnprClient::new(server.url()).resolve_projects(options2).await.unwrap_err();
    match coded_err {
        PnprClientError::Server { code, message } => {
            assert_eq!(code.as_deref(), Some("ERR_PNPR_GRAPH_TOO_LARGE"));
            assert!(message.contains("400"), "{message}");
        }
        other => panic!("expected Server error with code, got {other:?}"),
    }

    drop(handshake);
    resolve_mock.assert_async().await;
    verify_mock.assert_async().await;
    coded_mock.assert_async().await;
}
