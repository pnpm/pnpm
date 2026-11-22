use super::{
    PnprClient,
    PnprClientError,
};

impl PnprClient {
    pub(super) async fn resolution_post(
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
        let http = self.authenticated_http.get_or_try_init(|| async {
            let builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
            #[cfg(not(target_family = "wasm"))]
            let builder =
                if reqwest::Url::parse(&self.base_url).is_ok_and(|url| url.scheme() == "http") {
                    // Loopback HTTP must reach the local server directly. With redirects
                    // disabled, this connection never uses TLS.
                    builder
                        .no_proxy()
                        .tls_certs_only(std::iter::empty())
                } else {
                    builder
                };
            builder.build()
        })
        .await?;
        Ok(http.post(url).header(reqwest::header::AUTHORIZATION, value))
    }
}
