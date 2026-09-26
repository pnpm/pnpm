mod error;
mod request;
mod response;
pub(crate) mod types;

pub use error::PackageProviderError;
pub(crate) use request::build_provider_request;
pub(crate) use response::{invoke_provider, parse_provider_response, validate_provider_response};
pub(crate) use types::*;
pub use types::{PackageProviderInputs, PackageProviderOutput};

/// Send the dependency graph to the configured package provider and
/// return the directory it materialized each snapshot at, plus the
/// optional snapshots it skipped.
pub async fn materialize_through_package_provider(
    inputs: &PackageProviderInputs<'_>,
) -> Result<PackageProviderOutput, PackageProviderError> {
    let Some(bundle) = build_provider_request(inputs)? else {
        return Ok(PackageProviderOutput::default());
    };
    let request_json =
        serde_json::to_string(&bundle.request).map_err(PackageProviderError::SerializeRequest)?;
    let provider = inputs.package_provider.to_string();
    let stdout =
        tokio::task::spawn_blocking(move || invoke_provider(&provider, request_json.into_bytes()))
            .await
            .expect("package provider invocation must not panic")?;
    let response = parse_provider_response(inputs.package_provider, &stdout)?;
    validate_provider_response(&bundle, response)
}

#[cfg(test)]
mod tests;
