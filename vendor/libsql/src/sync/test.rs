use super::*;
use crate::util::Socket;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{duplex, AsyncRead, AsyncWrite, DuplexStream};
use tower::Service;

#[tokio::test]
async fn test_sync_context_push_frame() {
    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    let frame = Bytes::from("test frame data");
    let mut sync_ctx = sync_ctx;

    // Push a frame and verify the response
    let durable_frame = sync_ctx.push_frames(frame, 1, 0, 1, None).await.unwrap();
    sync_ctx.write_metadata().await.unwrap();
    assert_eq!(durable_frame.max_frame_no, 0); // First frame should return max_frame_no = 0

    // Verify internal state was updated
    assert_eq!(sync_ctx.durable_frame_num(), 0);
    assert_eq!(sync_ctx.durable_generation(), 1);
    assert_eq!(server.frame_count(), 1);
}

#[tokio::test]
async fn test_sync_context_with_auth() {
    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        Some("test_token".to_string()),
        None,
    )
    .await
    .unwrap();

    let frame = Bytes::from("test frame with auth");
    let mut sync_ctx = sync_ctx;

    let durable_frame = sync_ctx.push_frames(frame, 1, 0, 1, None).await.unwrap();
    sync_ctx.write_metadata().await.unwrap();
    assert_eq!(durable_frame.max_frame_no, 0);
    assert_eq!(server.frame_count(), 1);
}

#[tokio::test]
async fn test_sync_context_multiple_frames() {
    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    let mut sync_ctx = sync_ctx;

    // Push multiple frames and verify incrementing frame numbers
    for i in 0..3 {
        let frame = Bytes::from(format!("frame data {}", i));
        let durable_frame = sync_ctx.push_frames(frame, 1, i, 1, None).await.unwrap();
        sync_ctx.write_metadata().await.unwrap();
        assert_eq!(durable_frame.max_frame_no, i);
        assert_eq!(sync_ctx.durable_frame_num(), i);
        assert_eq!(server.frame_count(), i + 1);
    }
}

#[tokio::test]
async fn test_sync_context_corrupted_metadata() {
    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    // Create initial sync context and push a frame
    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    let mut sync_ctx = sync_ctx;
    let frame = Bytes::from("test frame data");
    let durable_frame = sync_ctx.push_frames(frame, 1, 0, 1, None).await.unwrap();
    sync_ctx.write_metadata().await.unwrap();
    assert_eq!(durable_frame.max_frame_no, 0);
    assert_eq!(server.frame_count(), 1);

    // Update metadata path to use -info instead of .meta
    let metadata_path = format!("{}-info", db_path.to_str().unwrap());
    std::fs::write(&metadata_path, b"invalid json data").unwrap();

    // Create new sync context with corrupted metadata
    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await;

    assert!(sync_ctx.is_err());
}

#[tokio::test]
async fn test_sync_restarts_with_lower_max_frame_no() {
    let _ = tracing_subscriber::fmt::try_init();

    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    // Create initial sync context and push a frame
    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    let mut sync_ctx = sync_ctx;
    let frame = Bytes::from("test frame data");
    let durable_frame = sync_ctx
        .push_frames(frame.clone(), 1, 0, 1, None)
        .await
        .unwrap();
    sync_ctx.write_metadata().await.unwrap();
    assert_eq!(durable_frame.max_frame_no, 0);
    assert_eq!(server.frame_count(), 1);

    // Bump the durable frame num so that the next time we call the
    // server we think we are further ahead than the database we are talking to is.
    sync_ctx.durable_frame_num += 3;
    sync_ctx.write_metadata().await.unwrap();

    // Create new sync context with corrupted metadata
    let mut sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    // Verify that the context was set to new fake values.
    assert_eq!(sync_ctx.durable_frame_num(), 3);
    assert_eq!(sync_ctx.durable_generation(), 1);

    let frame_no = sync_ctx.durable_frame_num() + 1;
    // This push should fail because we are ahead of the server and thus should get an invalid
    // frame no error.
    sync_ctx
        .push_frames(frame.clone(), 1, frame_no, 1, None)
        .await
        .unwrap_err();

    let frame_no = sync_ctx.durable_frame_num() + 1;
    // This then should work because when the last one failed it updated our state of the server
    // durable_frame_num and we should then start writing from there.
    sync_ctx
        .push_frames(frame, 1, frame_no, 1, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_sync_context_retry_on_error() {
    // Pause time to control it manually
    tokio::time::pause();

    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    let sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    let mut sync_ctx = sync_ctx;
    let frame = Bytes::from("test frame data");

    // Set server to return errors
    server.return_error.store(true, Ordering::SeqCst);

    // First attempt should fail but retry
    let result = sync_ctx.push_frames(frame.clone(), 1, 0, 1, None).await;
    assert!(result.is_err());

    // Advance time to trigger retries faster
    tokio::time::advance(Duration::from_secs(2)).await;

    // Verify multiple requests were made (retries occurred)
    assert!(server.request_count() > 1);

    // Allow the server to succeed
    server.return_error.store(false, Ordering::SeqCst);

    // Next attempt should succeed
    let durable_frame = sync_ctx.push_frames(frame, 1, 0, 1, None).await.unwrap();
    sync_ctx.write_metadata().await.unwrap();
    assert_eq!(durable_frame.max_frame_no, 0);
    assert_eq!(server.frame_count(), 1);
}

#[tokio::test]
async fn test_bootstrap_db_downloads_export() {
    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("bootstrap.db");

    // Seed metadata so SyncContext can be constructed (generation=1)
    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    let mut sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();


    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-info", db_path.to_str().unwrap()));

    // Bootstrap should fetch /info and then /export/{generation}
    crate::sync::bootstrap_db(&mut sync_ctx).await.unwrap();

    assert!(std::path::Path::new(&db_path).exists());
    assert!(std::path::Path::new(&format!("{}-info", db_path.to_str().unwrap())).exists());

    assert_eq!(sync_ctx.durable_generation(), 1);
    assert_eq!(sync_ctx.durable_frame_num(), 0);

    assert!(server.request_count() >= 2);
}

#[tokio::test]
async fn test_bootstrap_db_is_idempotent() {
    let server = MockServer::start();
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("bootstrap2.db");


    gen_metadata_file(&db_path, 3278479626, 0, 0, 1);

    let mut sync_ctx = SyncContext::new(
        server.connector(),
        db_path.to_str().unwrap().to_string(),
        server.url(),
        None,
        None,
    )
    .await
    .unwrap();

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-info", db_path.to_str().unwrap()));


    crate::sync::bootstrap_db(&mut sync_ctx).await.unwrap();
    let first_requests = server.request_count();

    // Second bootstrap should be a no-op (no new network calls)
    crate::sync::bootstrap_db(&mut sync_ctx).await.unwrap();
    let second_requests = server.request_count();
    assert_eq!(first_requests, second_requests);
}

#[test]
fn test_hash_verification() {
    let mut metadata = MetadataJson {
        hash: 0,
        version: 1,
        durable_frame_num: 100,
        generation: 5,
    };

    assert!(metadata.verify_hash().is_err());

    metadata.set_hash();

    assert!(metadata.verify_hash().is_ok());
}

#[test]
fn test_hash_tampering() {
    let mut metadata = MetadataJson {
        hash: 0,
        version: 1,
        durable_frame_num: 100,
        generation: 5,
    };

    // Create metadata with hash
    metadata.set_hash();

    // Tamper with a field
    metadata.version = 2;

    // Verify should fail
    assert!(metadata.verify_hash().is_err());

    metadata.version = 1;
    metadata.generation = 42;

    assert!(metadata.verify_hash().is_err());

    metadata.generation = 5;
    metadata.durable_frame_num = 42;

    assert!(metadata.verify_hash().is_err());

    metadata.durable_frame_num = 100;

    assert!(metadata.verify_hash().is_ok());
}

// Mock connector service that implements tower::Service
#[derive(Clone)]
struct MockConnector {
    tx: tokio::sync::mpsc::Sender<DuplexStream>,
}

impl Service<http::Uri> for MockConnector {
    type Response = Box<dyn Socket>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<Self::Response, Self::Error>>
                + Send,
        >,
    >;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: http::Uri) -> Self::Future {
        let (stream, server_stream) = duplex(1024);
        let _ = self.tx.try_send(server_stream);
        let conn = MockConnection { stream };
        Box::pin(std::future::ready(Ok(Box::new(conn) as Box<dyn Socket>)))
    }
}

#[allow(dead_code)]
struct MockServer {
    url: String,
    frame_count: Arc<AtomicU32>,
    connector: ConnectorService,
    return_error: Arc<AtomicBool>,
    request_count: Arc<AtomicU32>,
    export_bytes: Arc<Vec<u8>>, // bytes returned by /export/{generation}
}

impl MockServer {
    fn start() -> Self {
        let frame_count = Arc::new(AtomicU32::new(0));
        let return_error = Arc::new(AtomicBool::new(false));
        let request_count = Arc::new(AtomicU32::new(0));

        let export_bytes: Arc<Vec<u8>> = {
            use crate::local::Database;
            use crate::database::OpenFlags;
            use std::fs;
            use tempfile::NamedTempFile;

            let tmp = NamedTempFile::new().expect("temp file for export db");
            let path = tmp.path().to_path_buf();
            let db = Database::open(path.to_str().unwrap().to_string(), OpenFlags::default())
                .expect("open export db");
            let conn = db.connect().expect("connect export db");

            let _ = conn.query("CREATE TABLE IF NOT EXISTS t(x INTEGER);", crate::params::Params::None);
            drop(conn);
            drop(db);
            let bytes = fs::read(&path).expect("read export db bytes");
            Arc::new(bytes)
        };

        // Create the mock connector with Some(client_stream)
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mock_connector = MockConnector { tx };
        let connector = ConnectorService::new(mock_connector);

        let server = Self {
            url: "http://mock.server".to_string(),
            frame_count: frame_count.clone(),
            connector,
            return_error: return_error.clone(),
            request_count: request_count.clone(),
            export_bytes: export_bytes.clone(),
        };

        // Spawn the server handler
        let frame_count_clone = frame_count.clone();
        let return_error_clone = return_error.clone();
        let request_count_clone = request_count.clone();
        let export_bytes_clone = export_bytes.clone();

        tokio::spawn(async move {
            while let Some(server_stream) = rx.recv().await {
                let frame_count_clone = frame_count_clone.clone();
                let return_error_clone = return_error_clone.clone();
                let request_count_clone = request_count_clone.clone();
                let export_bytes_clone = export_bytes_clone.clone();

                tokio::spawn(async move {
                    use hyper::server::conn::Http;
                    use hyper::service::service_fn;

                    let frame_count_clone = frame_count_clone.clone();
                    let return_error_clone = return_error_clone.clone();
                    let request_count_clone = request_count_clone.clone();
                    let service = service_fn(move |req: http::Request<Body>| {
                        let frame_count = frame_count_clone.clone();
                        let return_error = return_error_clone.clone();
                        let request_count = request_count_clone.clone();
                        let export_bytes = export_bytes_clone.clone();
                        async move {
                            request_count.fetch_add(1, Ordering::SeqCst);
                            if return_error.load(Ordering::SeqCst) {
                                return Ok::<_, hyper::Error>(
                                    http::Response::builder()
                                        .status(500)
                                        .body(Body::from("Internal Server Error"))
                                        .unwrap(),
                                );
                            }

                            if req.uri().path().contains("/sync/") {
                                // Count only sync requests as frames to keep tests stable.
                                let current_count = frame_count.fetch_add(1, Ordering::SeqCst);
                                // Return the max_frame_no that has been accepted
                                let response = serde_json::json!({
                                    "status": "ok",
                                    "generation": 1,
                                    "max_frame_no": current_count
                                });

                                Ok::<_, hyper::Error>(
                                    http::Response::builder()
                                        .status(200)
                                        .body(Body::from(response.to_string()))
                                        .unwrap(),
                                )
                            } else if req.uri().path().eq("/info") {
                                let response = serde_json::json!({
                                    "current_generation": 1
                                });
                                Ok::<_, hyper::Error>(
                                    http::Response::builder()
                                        .status(200)
                                        .body(Body::from(response.to_string()))
                                        .unwrap(),
                                )
                            } else if req.uri().path().starts_with("/export/") {
                                Ok::<_, hyper::Error>(
                                    http::Response::builder()
                                        .status(200)
                                        .body(Body::from(export_bytes.as_ref().clone()))
                                        .unwrap(),
                                )
                            } else {
                                Ok(http::Response::builder()
                                    .status(404)
                                    .body(Body::empty())
                                    .unwrap())
                            }
                        }
                    });

                    if let Err(e) = Http::new().serve_connection(server_stream, service).await {
                        eprintln!("Error serving connection: {}", e);
                    }
                });
            }
        });

        server
    }

    fn connector(&self) -> ConnectorService {
        self.connector.clone()
    }

    fn url(&self) -> String {
        self.url.clone()
    }

    fn frame_count(&self) -> u32 {
        self.frame_count.load(Ordering::SeqCst)
    }

    fn request_count(&self) -> u32 {
        self.request_count.load(Ordering::SeqCst)
    }
}

// Mock connection that implements the Socket trait
struct MockConnection {
    stream: DuplexStream,
}

impl AsyncRead for MockConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for MockConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl hyper::client::connect::Connection for MockConnection {
    fn connected(&self) -> hyper::client::connect::Connected {
        hyper::client::connect::Connected::new()
    }
}

fn gen_metadata_file(db_path: &Path, hash: u32, version: u32, durable_frame_num: u32, generation: u32) {
    let metadata_path = format!("{}-info", db_path.to_str().unwrap());
    std::fs::write(
        &metadata_path,
        format!(
            "{{\"hash\": {hash}, \"version\": {version}, \"durable_frame_num\": {durable_frame_num}, \"generation\": {generation}}}"
        )
        .as_bytes(),
    )
    .unwrap();
}
