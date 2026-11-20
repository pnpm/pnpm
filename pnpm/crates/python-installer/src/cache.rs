use miette::{
    IntoDiagnostic,
    Result,
    bail,
};
use serde::de::DeserializeOwned;
use std::path::Path;
use tokio::io::AsyncReadExt;

pub(super) async fn read_json<Cached: DeserializeOwned>(
    path: &Path,
    limit: usize,
    description: &str,
) -> Result<Cached> {
    let file = tokio::fs::File::open(path).await.into_diagnostic()?;
    let mut contents = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut contents)
        .await
        .into_diagnostic()?;
    if contents.len() > limit {
        bail!("{description} exceeds {limit} bytes");
    }
    serde_json::from_slice(&contents).into_diagnostic()
}
