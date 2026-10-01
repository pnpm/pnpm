use crate::{
    Certificate, Error, Identity, Method, Proxy, Request, RequestBuilder, Response, Result,
    dns::Resolve,
    error::Kind,
    header::{HeaderMap, HeaderValue, USER_AGENT},
    redirect::Policy,
};
use std::{net::IpAddr, sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub struct Client {
    pub(crate) settings: Arc<Settings>,
}
#[derive(Debug)]
pub(crate) struct Settings {
    pub headers: HeaderMap,
    pub timeout: Duration,
    pub total_timeout: Option<Duration>,
    pub redirect: Policy,
}
#[derive(Debug)]
#[must_use]
pub struct ClientBuilder {
    settings: Settings,
    unsupported: Option<&'static str>,
}

impl Client {
    #[must_use]
    pub fn new() -> Self {
        Self::builder().build().expect("default host HTTP client")
    }
    pub fn builder() -> ClientBuilder {
        ClientBuilder {
            settings: Settings {
                headers: HeaderMap::new(),
                timeout: Duration::from_mins(1),
                total_timeout: None,
                redirect: Policy::default(),
            },
            unsupported: None,
        }
    }
    pub fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
        RequestBuilder::new(self.clone(), method, url.as_ref())
    }
    pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::GET, url)
    }
    pub fn head(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }
    pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::POST, url)
    }
    pub fn put(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::PUT, url)
    }
    pub fn delete(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }
    pub async fn execute(&self, request: Request) -> Result<Response> {
        request.send(self).await
    }
}
impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientBuilder {
    pub fn build(self) -> Result<Client> {
        if let Some(option) = self.unsupported {
            return Err(Error::new(
                Kind::Builder,
                format!("{option} is unavailable in WebContainers"),
            ));
        }
        Ok(Client { settings: Arc::new(self.settings) })
    }
    pub fn default_headers(mut self, headers: HeaderMap) -> Self {
        self.settings.headers = headers;
        self
    }
    pub fn user_agent(mut self, value: impl AsRef<str>) -> Self {
        match HeaderValue::from_str(value.as_ref()) {
            Ok(value) => {
                self.settings.headers.insert(USER_AGENT, value);
            }
            Err(_) => {
                self.unsupported = Some("Invalid User-Agent header");
            }
        }
        self
    }
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.settings.timeout = timeout;
        self
    }
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.settings.timeout = timeout;
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.settings.total_timeout = Some(timeout);
        self
    }
    pub fn redirect(mut self, policy: Policy) -> Self {
        self.settings.redirect = policy;
        self
    }
    pub fn no_proxy(self) -> Self {
        self
    }
    pub fn proxy(mut self, _proxy: Proxy) -> Self {
        self.unsupported = Some("Explicit HTTP proxy configuration");
        self
    }
    pub fn dns_resolver<R: Resolve + ?Sized + 'static>(mut self, resolver: Arc<R>) -> Self {
        if !resolver.uses_host_resolution() {
            self.unsupported = Some("Custom DNS resolution and address pinning");
        }
        drop(resolver);
        self
    }
    pub fn add_root_certificate(mut self, certificate: Certificate) -> Self {
        drop(certificate.into_der());
        self.unsupported = Some("Custom TLS trust roots");
        self
    }
    pub fn tls_certs_only(mut self, _certificates: impl IntoIterator<Item = Certificate>) -> Self {
        self.unsupported = Some("Custom TLS trust roots");
        self
    }
    pub fn identity(mut self, _identity: Identity) -> Self {
        self.unsupported = Some("Client TLS identities");
        self
    }
    pub fn danger_accept_invalid_certs(mut self, enabled: bool) -> Self {
        if enabled {
            self.unsupported = Some("Disabled TLS verification");
        }
        self
    }
    pub fn tls_danger_accept_invalid_hostnames(mut self, enabled: bool) -> Self {
        if enabled {
            self.unsupported = Some("Disabled TLS hostname verification");
        }
        self
    }
    pub fn local_address(mut self, _address: impl Into<Option<IpAddr>>) -> Self {
        self.unsupported = Some("Pinned local network addresses");
        self
    }
    // Browser networking owns protocol negotiation and connection pooling.
    pub fn http1_only(self) -> Self {
        self
    }
    pub fn gzip(self, _enabled: bool) -> Self {
        self
    }
    pub fn pool_idle_timeout(self, _timeout: impl Into<Option<Duration>>) -> Self {
        self
    }
    pub fn pool_max_idle_per_host(self, _maximum: usize) -> Self {
        self
    }
    pub fn tcp_keepalive(self, _timeout: impl Into<Option<Duration>>) -> Self {
        self
    }
}
