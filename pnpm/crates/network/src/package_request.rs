use reqwest::{
    RequestBuilder,
    Response,
};

use crate::{
    AuthHeaders,
    RetryOpts,
    ThrottledClient,
    ThrottledClientGuard,
    retry::RequestRetry,
};

/// A registry metadata request and the package whose route supplies its headers.
pub struct PackageRequest<'a> {
    pub url: &'a str,
    pub package: &'a str,
    pub auth: &'a AuthHeaders,
    pub headers: reqwest::header::HeaderMap,
    pub method: reqwest::Method,
    pub priority: u64,
}

impl PackageRequest<'_> {
    pub async fn send<'client>(
        &self,
        client: &'client ThrottledClient,
        retry: RetryOpts,
    ) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
        let mut retry = RequestRetry::new(self.url, retry);
        loop {
            if let Some(result) = retry.finish_attempt(self.send_once(client).await).await {
                return result;
            }
        }
    }

    pub async fn send_once<'client>(
        &self,
        client: &'client ThrottledClient,
    ) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
        let Some(credentials) = self.auth.managed_request_headers(self.url, Some(self.package))
        else {
            let guard = client.acquire_for_url_with_priority(self.url, self.priority).await;
            let request = crate::auth::apply_authorization(
                guard.request(self.method.clone(), self.url).headers(self.headers.clone()),
                self.auth.for_url_with_package(self.url, Some(self.package)),
            );
            let response = request
                .send()
                .await
                .inspect_err(|error| client.downscale_on_timeout(self.url, error))?;
            return Ok((guard, response));
        };
        let configure = |request: RequestBuilder, url: &str| {
            request
                .headers(self.headers.clone())
                .headers(credentials(url))
        };
        let (response, guard) = client.package_response_with_scoped_headers(
            self.method.clone(),
            self.url,
            self.priority,
            configure,
        )
        .await?;
        Ok((guard, response))
    }
}
