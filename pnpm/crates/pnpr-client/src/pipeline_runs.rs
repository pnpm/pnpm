//! Recording `pnpm pipeline` runs on a pnpr server.

use super::{Client, PnprClient, PnprClientError, Serialize, response_body_bounded};

/// One `pnpm pipeline` run as `PUT /-/pnpr/v0/pipeline/runs` carries it:
/// the workspace and run identifiers plus the run's summary document and
/// event stream, verbatim.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPipelineRunRequest {
    /// The organization the run is recorded under.
    pub org: String,
    pub workspace: String,
    pub run_id: String,
    pub summary: serde_json::Value,
    pub events: Vec<serde_json::Value>,
}

impl PnprClient {
    /// Record one `pnpm pipeline` run on the server. The document is
    /// stored verbatim; a server without the pipeline surface answers 404.
    pub async fn publish_pipeline_run(
        &self,
        request: &PublishPipelineRunRequest,
        authorization: Option<&str>,
    ) -> Result<(), PnprClientError> {
        if authorization.is_some() && !pnpm_network::is_url_secure_for_credentials(&self.base_url) {
            return Err(PnprClientError::Protocol(
                "pipeline report credentials require HTTPS or a loopback server".to_string(),
            ));
        }
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut put = http
            .put(format!("{}-/pnpr/v0/pipeline/runs", self.base_url))
            .timeout(self.artifact_request_timeout)
            .json(request);
        if let Some(authorization) = authorization {
            put = put.header("authorization", authorization);
        }
        let response = put.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response_body_bounded(response, 64 * 1024).await?;
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/pipeline/runs returned {status}: {}",
                String::from_utf8_lossy(&body),
            )));
        }
        Ok(())
    }
}
