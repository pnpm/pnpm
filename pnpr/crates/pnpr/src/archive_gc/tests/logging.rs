use super::{MANIFEST_LIMIT, blob, collect, name, setup};
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
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .without_time()
            .with_writer(std::fs::File::create(&path).unwrap())
            .finish();
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
