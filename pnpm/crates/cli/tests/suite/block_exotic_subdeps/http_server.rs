use crate::_utils::gzipped_tarball;
use serde_json::json;
use ssri::{
    Algorithm,
    IntegrityOpts,
};
use std::{
    collections::HashMap,
    io::{
        Read,
        Write,
    },
    net::TcpListener,
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            AtomicUsize,
            Ordering,
        },
    },
    thread::{
        self,
        JoinHandle,
    },
    time::Duration,
};

pub(super) struct TestHttpServer {
    pub(super) base_url: String,
    pub(super) exotic_hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

struct RouteResponse {
    content_type: &'static str,
    body: Vec<u8>,
}

impl TestHttpServer {
    pub(super) fn start(
        exotic_tgz: Vec<u8>,
        reg_packages: Vec<(&str, &str, serde_json::Value)>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback HTTP server");
        listener.set_nonblocking(true).expect("set listener nonblocking");
        let base_url = format!("http://{}", listener.local_addr().expect("read local addr"));
        let routes = Arc::new(build_server_routes(&base_url, exotic_tgz, reg_packages));
        let exotic_hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let hits_clone = Arc::clone(&exotic_hits);
        let stop_clone = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            accept_server_connections(&listener, &routes, &hits_clone, &stop_clone);
        });
        Self { base_url, exotic_hits, stop, handle: Some(handle) }
    }
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn build_server_routes(
    base_url: &str,
    exotic_tgz: Vec<u8>,
    reg_packages: Vec<(&str, &str, serde_json::Value)>,
) -> HashMap<String, RouteResponse> {
    let mut routes: HashMap<String, RouteResponse> = HashMap::new();
    routes.insert(
        "/node/v22.0.0/node-v22.0.0-linux-x64.tar.gz".to_string(),
        RouteResponse { content_type: "application/octet-stream", body: exotic_tgz.clone() },
    );
    routes.insert(
        "/exotic.tgz".to_string(),
        RouteResponse { content_type: "application/octet-stream", body: exotic_tgz },
    );
    for (name, version, deps) in reg_packages {
        insert_registry_package_routes(&mut routes, base_url, name, version, &deps);
    }
    routes
}

fn insert_registry_package_routes(
    routes: &mut HashMap<String, RouteResponse>,
    base_url: &str,
    name: &str,
    version: &str,
    deps: &serde_json::Value,
) {
    let pkg_json = json!({
        "name": name,
        "version": version,
        "main": "index.js",
        "dependencies": deps,
    })
    .to_string();
    let tgz = gzipped_tarball(&[
        ("package/package.json", &pkg_json),
        ("package/index.js", "module.exports = 'ok';\n"),
    ]);
    let integrity = IntegrityOpts::new()
        .algorithm(Algorithm::Sha512)
        .chain(&tgz)
        .result()
        .to_string();
    let tgz_path = format!("/{name}/-/{name}-{version}.tgz");
    let packument = json!({
        "name": name,
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dependencies": deps,
                "dist": {
                    "tarball": format!("{base_url}{tgz_path}"),
                    "integrity": integrity,
                }
            }
        }
    })
    .to_string();
    routes.insert(
        format!("/{name}"),
        RouteResponse { content_type: "application/json", body: packument.into_bytes() },
    );
    routes.insert(tgz_path, RouteResponse { content_type: "application/octet-stream", body: tgz });
}

fn accept_server_connections(
    listener: &TcpListener,
    routes: &Arc<HashMap<String, RouteResponse>>,
    hits: &Arc<AtomicUsize>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                spawn_connection_handler(stream, Arc::clone(routes), Arc::clone(hits));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
}

fn spawn_connection_handler(
    mut stream: std::net::TcpStream,
    routes: Arc<HashMap<String, RouteResponse>>,
    hits: Arc<AtomicUsize>,
) {
    thread::spawn(move || {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let Some(path) = read_request_path(&mut stream) else { return };
        if path == "/exotic.tgz" {
            hits.fetch_add(1, Ordering::SeqCst);
        }
        let Some(route) = routes.get(&path) else {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            route.content_type,
            route.body.len(),
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&route.body);
    });
}

fn read_request_path(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 512];
    while !buf
        .windows(4)
        .any(|window| window == b"\r\n\r\n")
        && buf.len() < 16 * 1024
    {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&buf);
    let first_line = text.lines().next()?;
    let raw_target = first_line.split_whitespace().nth(1)?;
    Some(
        raw_target
            .split('?')
            .next()
            .unwrap_or(raw_target)
            .to_string(),
    )
}
