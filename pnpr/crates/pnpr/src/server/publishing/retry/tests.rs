use super::{MEMORY_REPLAY_THRESHOLD, ReplayBody, ReplayStorage, retry_reclaimed};
use axum::body::Bytes;
use pnpr_error::RegistryError;
use std::{cell::Cell, future::ready, sync::Arc};
use tempfile::TempDir;
use tokio::sync::Semaphore;

#[tokio::test]
async fn reclamation_retries_stop_after_four_retries() {
    let temp = TempDir::new().unwrap();
    let storage = ReplayStorage::new(temp.path());
    let attempts = Cell::new(0);
    let error = retry_reclaimed(&storage, Bytes::from_static(b"request"), |body| {
        assert_eq!(body.as_ref(), b"request");
        attempts.set(attempts.get() + 1);
        ready(Err::<(), _>(RegistryError::ArchiveReclaimed {
            package: "example-pkg".to_string(),
            artifact: "example-pkg-1.0.0.tgz".to_string(),
        }))
    })
    .await
    .unwrap_err();
    assert_eq!(attempts.get(), 5);
    assert!(matches!(error, RegistryError::ArchiveReclaimed { .. }));
}

#[tokio::test]
async fn unrelated_errors_are_returned_without_retrying() {
    let temp = TempDir::new().unwrap();
    let storage = ReplayStorage::new(temp.path());
    let attempts = Cell::new(0);
    let error = retry_reclaimed(&storage, Bytes::from_static(b"request"), |body| {
        assert_eq!(body.as_ref(), b"request");
        attempts.set(attempts.get() + 1);
        ready(Err::<(), _>(RegistryError::Forbidden {
            user: "publisher".to_string(),
            action: "publish",
            resource: "example-pkg".to_string(),
        }))
    })
    .await
    .unwrap_err();
    assert_eq!(attempts.get(), 1);
    assert!(matches!(error, RegistryError::Forbidden { .. }));
}

#[tokio::test]
async fn large_requests_release_the_raw_buffer_and_replay_from_disk() {
    let temp = TempDir::new().unwrap();
    let storage = ReplayStorage::new(temp.path());
    let attempts = Cell::new(0);
    let body = Bytes::from(vec![b'x'; MEMORY_REPLAY_THRESHOLD + 1]);
    retry_reclaimed(&storage, body, |body| {
        assert!(body.is_unique(), "no replay buffer retains the request bytes");
        assert_eq!(body.len(), MEMORY_REPLAY_THRESHOLD + 1);
        assert!(body.iter().all(|byte| *byte == b'x'));
        attempts.set(attempts.get() + 1);
        ready(if attempts.get() < 5 {
            Err(RegistryError::ArchiveReclaimed {
                package: "example-pkg".to_string(),
                artifact: "example-pkg-1.0.0.tgz".to_string(),
            })
        } else {
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(attempts.get(), 5);
}

#[tokio::test]
async fn concurrent_replays_share_a_byte_budget_until_their_files_are_dropped() {
    let temp = TempDir::new().unwrap();
    let length = MEMORY_REPLAY_THRESHOLD + 1;
    let storage = ReplayStorage {
        directory: temp.path().join("cache"),
        bytes: Arc::new(Semaphore::new(2 * length)),
    };
    let body = Bytes::from(vec![b'x'; length]);
    let first_replay = ReplayBody::new(&storage, &body);
    let second_replay = ReplayBody::new(&storage, &body);
    let third_replay = ReplayBody::new(&storage, &body);
    let outcomes: [_; 3] = tokio::join!(first_replay, second_replay, third_replay).into();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.is_ok())
            .count(),
        2,
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Err(RegistryError::TooManyRequests { .. })))
            .count(),
        1,
    );
    assert!(storage.directory.is_dir());
    assert_eq!(storage.bytes.available_permits(), 0);
    drop(outcomes);
    assert_eq!(storage.bytes.available_permits(), 2 * length);
    assert!(ReplayBody::new(&storage, &body).await.is_ok());
}

#[tokio::test]
async fn a_replay_file_failure_preserves_publishing_within_the_byte_budget() {
    let temp = TempDir::new().unwrap();
    let storage = ReplayStorage::new(temp.path());
    std::fs::write(&storage.directory, b"a file, not a directory").unwrap();
    let available = storage.bytes.available_permits();
    let length = MEMORY_REPLAY_THRESHOLD + 1;
    let attempts = Cell::new(0);
    let body = Bytes::from(vec![b'x'; length]);
    retry_reclaimed(&storage, body, |body| {
        assert_eq!(body.len(), length);
        assert!(body.iter().all(|byte| *byte == b'x'));
        assert_eq!(storage.bytes.available_permits(), available - length);
        attempts.set(attempts.get() + 1);
        ready(if attempts.get() == 1 {
            Err(RegistryError::ArchiveReclaimed {
                package: "example-pkg".to_string(),
                artifact: "example-pkg-1.0.0.tgz".to_string(),
            })
        } else {
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(attempts.get(), 2);
    assert_eq!(storage.bytes.available_permits(), available);
}

#[tokio::test]
async fn outstanding_replay_reads_hold_their_budget_after_request_cleanup() {
    let temp = TempDir::new().unwrap();
    let storage = ReplayStorage::new(temp.path());
    let available = storage.bytes.available_permits();
    let length = MEMORY_REPLAY_THRESHOLD + 1;
    let body = Bytes::from(vec![b'x'; length]);
    let replay = ReplayBody::new(&storage, &body).await.unwrap();
    let file = match &replay {
        ReplayBody::File { file, .. } => Arc::clone(file),
        ReplayBody::Memory { .. } => panic!("large replays use files"),
    };
    let (started, waiting) = tokio::sync::oneshot::channel();
    let (resume, paused) = std::sync::mpsc::channel();
    let reading = tokio::task::spawn_blocking(move || {
        started.send(()).unwrap();
        paused.recv().unwrap();
        file.read_bytes(length).unwrap()
    });
    waiting.await.unwrap();
    drop(replay);
    assert_eq!(storage.bytes.available_permits(), available - length);
    resume.send(()).unwrap();
    let bytes = reading.await.unwrap();
    assert_eq!(bytes.as_slice(), body.as_ref());
    assert_eq!(storage.bytes.available_permits(), available);
}
