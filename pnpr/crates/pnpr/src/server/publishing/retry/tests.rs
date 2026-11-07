use super::{MAX_MEMORY_REPLAY_BYTES, retry_reclaimed};
use axum::body::Bytes;
use pnpr_error::RegistryError;
use std::{cell::Cell, future::ready};

#[tokio::test]
async fn reclamation_retries_stop_after_four_retries() {
    let attempts = Cell::new(0);
    let error = retry_reclaimed(Bytes::from_static(b"request"), |body| {
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
    let attempts = Cell::new(0);
    let error = retry_reclaimed(Bytes::from_static(b"request"), |body| {
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
    let attempts = Cell::new(0);
    let body = Bytes::from(vec![b'x'; MAX_MEMORY_REPLAY_BYTES + 1]);
    retry_reclaimed(body, |body| {
        assert!(body.is_unique(), "no replay buffer retains the request bytes");
        assert_eq!(body.len(), MAX_MEMORY_REPLAY_BYTES + 1);
        assert!(body.iter().all(|byte| *byte == b'x'));
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
}
