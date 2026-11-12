use super::{MANIFEST_LIMIT, blob, collect, name, retained_image_with_layers, setup};
use pnpr_package_name::Ecosystem;
use std::time::Duration;

#[tokio::test]
async fn collection_logs_only_removed_archives_with_sizes_and_image_repositories() {
    for pre_unmapped in [false, true] {
        let (temp, storage) = setup();
        let repository = name("example/app");
        let digest = blob(&storage, &repository, b"orphan").await;
        let archive = storage
            .hosted_blob_of(&repository, &digest.blob_filename())
            .await
            .unwrap()
            .unwrap();
        if pre_unmapped {
            storage
                .sql_metadata()
                .unmap_artifacts(
                    "",
                    Ecosystem::Oci,
                    repository.as_str(),
                    (None, vec![digest.blob_filename().into()]),
                )
                .await
                .unwrap()
                .unwrap();
        }
        let path = temp.path().join("gc.log");
        let subscriber = log_subscriber(&path);
        let _default = tracing::subscriber::set_default(subscriber);
        assert_eq!(collect(&storage, Duration::ZERO, true, MANIFEST_LIMIT).await.unwrap(), (1, 6));
        assert!(!std::fs::read_to_string(&path).unwrap().contains("removed archive"));
        assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (1, 6));
        assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (0, 0));
        let output = std::fs::read_to_string(&path).unwrap();
        let removed: Vec<_> = output
            .lines()
            .filter(|line| line.contains("removed archive"))
            .collect();
        assert_eq!(removed.len(), 1, "{output}");
        assert!(removed[0].contains(&format!("key={}", archive.key())), "{output}");
        assert!(removed[0].contains("size=6"), "{output}");
        assert!(removed[0].contains("INFO"), "{output}");
        assert_eq!(removed[0].contains("repository=example/app"), !pre_unmapped, "{output}");
    }
}

#[tokio::test]
async fn collection_logs_successful_removals_when_another_archive_cannot_be_deleted() {
    for pre_unmapped in [false, true] {
        let (temp, storage) = setup();
        let repository = name("example/app");
        let good = blob(&storage, &repository, b"orphan").await;
        let bad = blob(&storage, &repository, b"blocked").await;
        let archive = storage
            .hosted_blob_of(&repository, &good.blob_filename())
            .await
            .unwrap()
            .unwrap();
        let blocked = super::archive_path(&temp, &storage, &repository, &bad).await;
        std::fs::remove_file(&blocked).unwrap();
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep"), b"blocked").unwrap();
        if pre_unmapped {
            storage
                .sql_metadata()
                .unmap_artifacts(
                    "",
                    Ecosystem::Oci,
                    repository.as_str(),
                    (None, vec![good.blob_filename().into(), bad.blob_filename().into()]),
                )
                .await
                .unwrap()
                .unwrap();
        }
        let path = temp.path().join("gc.log");
        let subscriber = log_subscriber(&path);
        let _default = tracing::subscriber::set_default(subscriber);
        assert!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.is_err());
        let output = std::fs::read_to_string(&path).unwrap();
        let removed: Vec<_> = output
            .lines()
            .filter(|line| line.contains("removed archive"))
            .collect();
        assert_eq!(removed.len(), 1, "{output}");
        assert!(removed[0].contains(&format!("key={}", archive.key())), "{output}");
        assert!(removed[0].contains("size=6"), "{output}");
        assert_eq!(removed[0].contains("repository=example/app"), !pre_unmapped, "{output}");
        let good_path = pnpr_storage::layout::local_archive_path(
            &temp.path().join("store"),
            "",
            Ecosystem::Oci,
            &archive.key(),
        );
        assert!(!good_path.exists());
        assert!(blocked.is_dir());
    }
}

#[tokio::test]
async fn collection_audit_logs_keep_namespace_context_at_warn_level() {
    let (temp, storage) = setup();
    let repository = name("example/app");
    for (namespace, pre_unmapped) in [("team-one", false), ("team-two", true)] {
        let view = storage.for_hosted(namespace, Ecosystem::Oci);
        let digest = blob(&view, &repository, b"orphan").await;
        if pre_unmapped {
            view.sql_metadata()
                .unmap_artifacts(
                    namespace,
                    Ecosystem::Oci,
                    repository.as_str(),
                    (None, vec![digest.blob_filename().into()]),
                )
                .await
                .unwrap()
                .unwrap();
        }
    }
    let yaml = "storage: ./store\ncache: ./cache\ndatabase: { url: 'sqlite:./metadata.sqlite' }\nregistries:\n  first: { type: hosted, ecosystem: oci, namespace: team-one, access: '$all' }\n  second: { type: hosted, ecosystem: oci, namespace: team-two, access: '$all' }\n";
    let config_path = temp.path().join("config.yaml");
    std::fs::write(&config_path, yaml).unwrap();
    let config =
        crate::Config::from_yaml(&config_path, "127.0.0.1:4873".parse().unwrap(), None).unwrap();
    let path = temp.path().join("gc.log");
    let logs = pnpr_config::LogConfig {
        format: pnpr_config::LogFormat::Json,
        level: pnpr_config::LogLevel::Warn,
    };
    let subscriber = crate::logging::log_subscriber(
        &logs,
        tracing_subscriber::EnvFilter::new(logs.level.as_filter_directive()),
        std::fs::File::create(&path).unwrap(),
        false,
    );
    let _default = tracing::subscriber::set_default(subscriber);
    let report =
        crate::archive_gc::collect_archives(&config, &crate::archive_gc::GcOptions::default())
            .await
            .unwrap();
    assert_eq!(report.archives, 2);
    let output = std::fs::read_to_string(path).unwrap();
    let removed: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|event: &serde_json::Value| event["fields"]["message"] == "removed archive")
        .collect();
    assert_eq!(removed.len(), 2, "{output}");
    assert_eq!(removed[0]["fields"]["key"], removed[1]["fields"]["key"]);
    for (event, namespace) in removed
        .iter()
        .zip(["team-one", "team-two"])
    {
        assert_eq!(event["span"]["namespace"], namespace);
        assert_eq!(event["span"]["ecosystem"], "oci");
        assert_eq!(
            event["fields"]["repository"].as_str(),
            (namespace == "team-one").then_some("example/app"),
        );
        assert_eq!(event["target"], "pnpr::audit");
        assert_eq!(event["fields"]["action"], "gc.remove_archive");
        assert_eq!(event["fields"]["actor"], "-");
    }
}

#[tokio::test]
async fn dry_run_logs_only_archives_no_other_repository_keeps() {
    let (temp, storage) = setup();
    let app = name("example/app");
    let worker = name("example/worker");
    let shared_bytes = b"shared layer";
    let app_image =
        retained_image_with_layers(&storage, &app, b"app config", &[shared_bytes]).await;
    retained_image_with_layers(&storage, &worker, b"worker config", &[shared_bytes]).await;
    let shared_archive = storage
        .hosted_blob_of(&worker, &app_image.layers[0].blob_filename())
        .await
        .unwrap()
        .unwrap();
    storage
        .update_hosted_document_with_retry(&app, 1, None, |_| Ok(Some(Vec::new())))
        .await
        .unwrap();
    let path = temp.path().join("gc.log");
    let subscriber = log_subscriber(&path);
    let _default = tracing::subscriber::set_default(subscriber);
    assert_eq!(
        collect(&storage, Duration::ZERO, true, MANIFEST_LIMIT).await.unwrap(),
        (2, app_image.manifest_and_config_bytes),
    );
    let output = std::fs::read_to_string(&path).unwrap();
    let unmapped = output
        .lines()
        .filter(|line| line.contains("unmapped archive"))
        .count();
    assert_eq!(unmapped, 2, "{output}");
    assert!(!output.contains(&shared_archive.key()), "{output}");
}

fn log_subscriber(path: &std::path::Path) -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .with_writer(std::fs::File::create(path).unwrap())
        .finish()
}
