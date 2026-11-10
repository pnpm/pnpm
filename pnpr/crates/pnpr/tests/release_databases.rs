//! End-to-end checks for built `pnpr` binaries against every `database.url`
//! scheme.

#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
mod db_guard;

#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use crate::npm::publish_doc;
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use db_guard::{create_mysql_database, create_postgres_database};
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use pnpm_testing_utils::trusted_tls_server::TrustedTlsServer;
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use pnpr_libsql::testing::{serve_primary, serve_tls_proxy};
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use reqwest::{Client, StatusCode};
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use serde_json::{Value, json};
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use std::{net::SocketAddr, process::Stdio};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;
#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
};
use tokio::{process::Command, time::timeout};

const STARTUP_DEADLINE: Duration = Duration::from_secs(30);

#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
struct SchemeCase {
    label: &'static str,
    url: String,
    auth_token: Option<&'static str>,
    tls_ca: Option<PathBuf>,
}

#[tokio::test]
#[ignore = "requires PNPR_TEST_SQLITE_ONLY_BIN"]
async fn sqlite_only_binary_rejects_unbuilt_database_schemes() {
    let bin = PathBuf::from(
        std::env::var("PNPR_TEST_SQLITE_ONLY_BIN")
            .expect("PNPR_TEST_SQLITE_ONLY_BIN must point to a --no-default-features pnpr binary"),
    );

    let sqlite_dir = TempDir::new().unwrap();
    let sqlite_cfg = write_config(sqlite_dir.path(), "sqlite:./metadata.sqlite", None);
    let (ok_code, stdout, stderr) =
        run_cli(&bin, &["--config", sqlite_cfg.to_str().unwrap(), "config", "check"]).await;
    assert_eq!(ok_code, Some(0), "sqlite config check failed:\nstdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.contains("Configuration check: OK"), "{stdout}");

    for url in [
        "libsql://database.example.test",
        "http://127.0.0.1:8080",
        "https://database.example.test",
        "postgres://example:supersecret@database.example.test/registry",
        "postgresql://example:supersecret@database.example.test/registry",
        "mysql://example:supersecret@database.example.test/registry",
    ] {
        let dir = TempDir::new().unwrap();
        let token = url.starts_with("libsql://").then_some("supersecret-token");
        let cfg = write_config(dir.path(), url, token);
        let cfg_str = cfg.to_str().unwrap();

        for subcommand in [
            vec!["--config", cfg_str, "config", "check"],
            vec!["--config", cfg_str, "serve", "--listen", "127.0.0.1:0"],
        ] {
            let (code, stdout, stderr) = run_cli(&bin, &subcommand).await;
            assert_ne!(code, Some(0), "{url} unexpectedly succeeded for {subcommand:?}");
            let combined = format!("{stdout}\n{stderr}");
            assert!(
                combined.contains("this build does not include the configured database driver"),
                "{url} ({subcommand:?}): {combined}",
            );
            assert!(!combined.contains("supersecret"), "{url} leaked secret: {combined}");
        }
    }
}

#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PNPR_TEST_RELEASE_BIN, PNPR_TEST_POSTGRES_URL, and PNPR_TEST_MYSQL_URL"]
async fn release_binary_supports_every_database_scheme() {
    let bin = PathBuf::from(
        std::env::var("PNPR_TEST_RELEASE_BIN")
            .expect("PNPR_TEST_RELEASE_BIN must point to the release pnpr binary"),
    );
    let pg_admin =
        std::env::var("PNPR_TEST_POSTGRES_URL").expect("PNPR_TEST_POSTGRES_URL is required");
    let mysql_admin =
        std::env::var("PNPR_TEST_MYSQL_URL").expect("PNPR_TEST_MYSQL_URL is required");

    let scratch = TempDir::new().unwrap();
    let (_http_primary, http_addr, _http_task) =
        serve_primary(scratch.path().join("http-primary.db")).await;
    let (_https_primary, https_upstream, _https_task) =
        serve_primary(scratch.path().join("https-primary.db")).await;
    let (_libsql_primary, libsql_upstream, _libsql_task) =
        serve_primary(scratch.path().join("libsql-primary.db")).await;

    let ca_path = TrustedTlsServer::ca_path();
    let cert_pem = std::fs::read(ca_path.with_file_name("server.crt")).unwrap();
    let key_pem = std::fs::read(ca_path.with_file_name("server.key")).unwrap();
    let (https_addr, _https_proxy) = serve_tls_proxy(https_upstream, &cert_pem, &key_pem).await;
    let (libsql_addr, _libsql_proxy) = serve_tls_proxy(libsql_upstream, &cert_pem, &key_pem).await;

    let pg_db = create_postgres_database(&pg_admin, "postgres").await;
    let postgresql_db = create_postgres_database(&pg_admin, "postgresql").await;
    let mysql_db = create_mysql_database(&mysql_admin).await;

    let cases = [
        SchemeCase {
            label: "sqlite",
            url: "sqlite:./metadata.sqlite".to_string(),
            auth_token: None,
            tls_ca: None,
        },
        SchemeCase {
            label: "http",
            url: format!("http://{http_addr}"),
            auth_token: None,
            tls_ca: None,
        },
        SchemeCase {
            label: "https",
            url: format!("https://{https_addr}"),
            auth_token: Some("test-libsql-token"),
            tls_ca: Some(ca_path.clone()),
        },
        SchemeCase {
            label: "libsql",
            url: format!("libsql://{libsql_addr}"),
            auth_token: Some("test-libsql-token"),
            tls_ca: Some(ca_path),
        },
        SchemeCase { label: "postgres", url: pg_db.url.clone(), auth_token: None, tls_ca: None },
        SchemeCase {
            label: "postgresql",
            url: postgresql_db.url.clone(),
            auth_token: None,
            tls_ca: None,
        },
        SchemeCase { label: "mysql", url: mysql_db.url.clone(), auth_token: None, tls_ca: None },
    ];

    let client = Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    for case in &cases {
        exercise_scheme(&bin, &client, case).await;
    }
}

#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
async fn exercise_scheme(bin: &Path, client: &Client, case: &SchemeCase) {
    let dir = TempDir::new().unwrap();
    let cfg = write_config(dir.path(), &case.url, case.auth_token);
    let cfg_str = cfg.to_str().unwrap();

    let (code, stdout, stderr) = run_cli(bin, &["--config", cfg_str, "config", "check"]).await;
    assert_eq!(
        code,
        Some(0),
        "{} config check failed:\nstdout: {stdout}\nstderr: {stderr}",
        case.label,
    );
    assert!(stdout.contains("Configuration check: OK"), "{}: {stdout}", case.label);

    let (mut child, listen) = spawn_server(bin, &cfg, case.tls_ca.as_deref(), case.label).await;
    let base = format!("http://{listen}");

    let ready = client
        .get(format!("{base}/-/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK, "{} ready probe failed", case.label);

    let signup = client
        .put(format!("{base}/-/user/org.couchdb.user:alice"))
        .json(&json!({ "name": "alice", "password": "secret123", "type": "user", "roles": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(signup.status(), StatusCode::CREATED, "{} user signup failed", case.label);
    let signup_json: Value = signup.json().await.unwrap();
    let token = signup_json["token"].as_str().expect("signup returns bearer token");

    let pkg = format!("smoke-{}", case.label);
    let publish = client
        .put(format!("{base}/npm/~local/{pkg}"))
        .bearer_auth(token)
        .json(&publish_doc(&pkg, "1.0.0", case.label.as_bytes()))
        .send()
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED, "{} publish failed", case.label);

    let fetched = client
        .get(format!("{base}/npm/~local/{pkg}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(fetched.status(), StatusCode::OK, "{} packument read failed", case.label);
    let packument: Value = fetched.json().await.unwrap();
    assert_eq!(packument["dist-tags"]["latest"], "1.0.0", "{}", case.label);

    child.kill().await.unwrap();
    let _ = child.wait().await;
}

fn write_config(dir: &Path, database_url: &str, auth_token: Option<&str>) -> PathBuf {
    let token_line = match auth_token {
        Some(token) => format!("  authToken: {token}\n"),
        None => String::new(),
    };
    let yaml = format!(
        "storage: ./storage\n\
         database:\n\
         \x20 url: {database_url}\n\
         {token_line}\
         auth:\n\
         \x20 users:\n\
         \x20   maxUsers: 10\n\
         registries:\n\
         \x20 local:\n\
         \x20   type: hosted\n\
         defaultRegistry: local\n\
         log:\n\
         \x20 format: json\n\
         \x20 level: info\n",
    );
    let path = dir.join("pnpr.yaml");
    std::fs::write(&path, yaml).unwrap();
    path
}

async fn run_cli(bin: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let output = timeout(
        STARTUP_DEADLINE,
        Command::new(bin)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("pnpr CLI did not exit before deadline")
    .expect("spawn pnpr CLI");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[cfg(all(feature = "backend-postgres", feature = "backend-mysql"))]
async fn spawn_server(
    bin: &Path,
    cfg: &Path,
    tls_ca: Option<&Path>,
    label: &str,
) -> (Child, SocketAddr) {
    let mut command = Command::new(bin);
    command
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--public-url",
            "http://example.test",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(ca) = tls_ca {
        command.env("SSL_CERT_FILE", ca);
    }
    let mut child = command.spawn().expect("spawn pnpr server");
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let mut seen_stdout = String::new();

    let listen = timeout(STARTUP_DEADLINE, async {
        while let Some(line) = lines.next_line().await.unwrap() {
            seen_stdout.push_str(&line);
            seen_stdout.push('\n');
            let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
            if record["fields"]["message"] == "pnpr listening" {
                let raw = record["fields"]["listen"].as_str().expect("listen address field");
                return Some(raw.parse::<SocketAddr>().expect("valid listen SocketAddr"));
            }
        }
        None
    })
    .await;

    match listen {
        Ok(Some(addr)) => {
            tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
            (child, addr)
        }
        other => {
            let _ = child.kill().await;
            let output = child.wait_with_output().await.unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!(
                "{label} server failed to listen ({other:?}):\nstdout:\n{seen_stdout}\nstderr:\n{stderr}",
            );
        }
    }
}
