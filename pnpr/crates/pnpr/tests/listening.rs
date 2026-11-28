use serde_json::Value;
use std::{
    net::SocketAddr,
    process::Stdio,
    time::Duration,
};
use tokio::io::{
    AsyncBufReadExt as _,
    BufReader,
};

#[tokio::test]
async fn listening_event_reports_the_address_serving_the_packument() {
    for public_url in [None, Some("https://registry.example/base")] {
        let storage = crate::storage::build_storage();
        let path = storage.path().join("pnpr.yaml");
        let yaml = "storage: .
log: {format: json}
registries:
  local:
    type: hosted
    namespace: ''
    access: $all
defaultRegistry: local
";
        crate::managed_registries::managed_config(
            &path,
            yaml,
            "127.0.0.1:0".parse().unwrap(),
            None,
        )
        .await;
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_pnpr"));
        command
            .args(["--listen", "127.0.0.1:0", "--config"])
            .arg(path)
            .env_remove("PNPR_PUBLIC_URL")
            .env("RUST_LOG", "pnpr::startup=info")
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        if let Some(url) = public_url {
            command.arg("--public-url").arg(url);
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "the test starts pnpr as a standalone server"
        )]
        let mut server = command.spawn().unwrap();
        let addr = tokio::time::timeout(
            Duration::from_secs(30),
            listening_address(server.stdout.take().unwrap()),
        )
        .await
        .expect("pnpr listening event");
        assert_ne!(addr.port(), 0);
        let origin = format!("http://{addr}");
        let response = reqwest::get(format!("{origin}/npm/@foo/no-deps")).await.unwrap();
        assert!(response.status().is_success(), "{}", response.text().await.unwrap());
        let document: Value = response.json().await.unwrap();
        let url = document["versions"]["1.0.0"]["dist"]["tarball"].as_str().unwrap();
        assert_eq!(url, format!("{}/npm/@foo/no-deps@1.0.0", public_url.unwrap_or(&origin)));
        if public_url.is_none() {
            assert_eq!(
                reqwest::get(url).await
                    .unwrap()
                    .bytes()
                    .await
                    .unwrap()
                    .as_ref(),
                b"fake @foo/no-deps tarball",
            );
        }
        server.kill().await.unwrap();
    }
}

async fn listening_address(stdout: tokio::process::ChildStdout) -> SocketAddr {
    let mut records = BufReader::new(stdout).lines();
    loop {
        let line = records
            .next_line()
            .await
            .unwrap()
            .expect("pnpr exited before listening");
        let record: Value = serde_json::from_str(&line).unwrap();
        if record["fields"]["event"] == "listening" {
            return record["fields"]["listen"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
        }
    }
}
