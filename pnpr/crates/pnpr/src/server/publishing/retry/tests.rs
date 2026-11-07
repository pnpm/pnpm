use super::retry_reclaimed;
use pnpr_error::RegistryError;
use std::{cell::Cell, future::ready};

#[tokio::test]
async fn reclamation_retries_stop_after_four_retries() {
    let attempts = Cell::new(0);
    let error = retry_reclaimed(|| {
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
    let error = retry_reclaimed(|| {
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
