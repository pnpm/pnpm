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
        let Some(credentials) = self.auth.managed_request_headers(self.url, self.package) else {
            let guard = client.acquire_for_url_with_priority(self.url, self.priority).await;
            let mut request =
                guard.request(self.method.clone(), self.url).headers(self.headers.clone());
            if let Some(value) = self.auth.for_url_with_package(self.url, Some(self.package)) {
                request = request.header(reqwest::header::AUTHORIZATION, value);
            }
            let response = request
                .send()
                .await
                .inspect_err(|error| client.downscale_on_timeout(self.url, error))?;
            return Ok((guard, response));
        };
        let origin = reqwest::Url::parse(self.url).ok().map(|url| url.origin());
        let configure = |request: RequestBuilder, url: &str| {
            let request = request.headers(self.headers.clone());
            if url == self.url {
                return request.headers(credentials.clone());
            }
            if !credentials.is_empty()
                && reqwest::Url::parse(url).is_ok_and(|url| Some(url.origin()) == origin)
                && self.auth.metadata_scope(url, Some(self.package))
                    == self.auth.metadata_scope(self.url, Some(self.package))
                && let Some(headers) = self.auth.managed_request_headers(url, self.package)
            {
                return request.headers(headers);
            }
            request
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
