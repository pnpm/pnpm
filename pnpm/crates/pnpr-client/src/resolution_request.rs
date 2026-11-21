use super::{
    PnprClient,
    PnprClientError,
};

impl PnprClient {
    pub(super) fn resolution_post(
        &self,
        path: &str,
        authorization: Option<&str>,
    ) -> Result<reqwest::RequestBuilder, PnprClientError> {
        let url = format!("{}{path}", self.base_url);
        let Some(value) = authorization else { return Ok(self.http.post(url)) };
        if !pnpm_network::is_url_secure_for_credentials(&self.base_url) {
            return Err(PnprClientError::Protocol(
                "pnpr server credentials require HTTPS or a loopback server".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(http.post(url).header(reqwest::header::AUTHORIZATION, value))
    }
}
