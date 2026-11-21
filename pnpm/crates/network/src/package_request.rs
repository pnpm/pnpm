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
        let managed = self.auth.managed_request_headers(self.url, Some(self.package));
        let authorize = managed
            .is_none()
            .then(|| {
                crate::scoped_redirect_auth(self.url, |url| {
                    self.auth.for_url_with_package(url, Some(self.package))
                })
            });
        let configure = |request: RequestBuilder, url: &str| {
            let request = request.headers(self.headers.clone());
            match &managed {
                Some(credentials) => request.headers(credentials(url)),
                None => crate::auth::apply_authorization(
                    request,
                    authorize.as_ref().and_then(|lookup| lookup(url)),
                ),
            }
        };
        let options = crate::ScopedGetOptions { priority: self.priority, ..Default::default() };
        let (response, guard) = client
            .response_with_scoped_headers(
                self.method.clone(),
                self.url,
                None,
                None,
                options,
                configure,
            )
            .await
            .map_err(crate::HeadError::into_request_error)?;
        Ok((guard, response))
    }
}
