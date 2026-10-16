//! The outbound HTTP the `openidconnect` crate performs through this state:
//! every request is bounded to a public destination and a 1 MiB body.

use super::{
    HttpRequest, HttpResponse, OidcState, RegistryError, Result, network, secure_url, unavailable,
};

impl OidcState {
    async fn http_request(&self, request: HttpRequest) -> Result<HttpResponse> {
        secure_url(&request.uri().to_string())?;
        network::validate_destination(&request.uri().to_string())?;
        let (parts, body) = request.into_parts();
        let mut response = self.http
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| unavailable())?;
        let mut builder = openidconnect::http::Response::builder().status(response.status());
        for (key, value) in response.headers() {
            builder = builder.header(key, value);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if body.len() + chunk.len() > 1024 * 1024 {
                return Err(unavailable());
            }
            body.extend_from_slice(&chunk);
        }
        builder.body(body).map_err(|_| unavailable())
    }
}

impl<'client> openidconnect::AsyncHttpClient<'client> for OidcState {
    type Error = RegistryError;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<HttpResponse>> + Send + 'client>>;

    fn call(&'client self, request: HttpRequest) -> Self::Future {
        Box::pin(self.http_request(request))
    }
}
