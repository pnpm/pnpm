use pnpr_error::RegistryError;
use std::future::Future;

/// Retry a request whose archives a concurrent reclaim fenced off. Each
/// attempt must validate and stage the original request again, including its
/// authorization and tombstone checks, before choosing new incarnations.
pub(in super::super) async fn retry_reclaimed<
    Output,
    Attempt: Future<Output = Result<Output, RegistryError>>,
>(
    mut attempt: impl FnMut() -> Attempt,
) -> Result<Output, RegistryError> {
    let mut retries = 0;
    loop {
        match attempt().await {
            Err(RegistryError::ArchiveReclaimed { .. }) if retries < 4 => retries += 1,
            outcome => return outcome,
        }
    }
}

#[cfg(test)]
mod tests;
