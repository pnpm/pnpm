use crate::{
    Client, Error, Method, Response, Result, Url,
    error::Kind,
    header::{
        AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderMap, HeaderName, HeaderValue,
        LOCATION, PROXY_AUTHORIZATION, WWW_AUTHENTICATE,
    },
    redirect::Action,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Request {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
    timeout: Option<Duration>,
}
#[derive(Debug)]
#[must_use]
pub struct RequestBuilder {
    client: Client,
    request: Result<Request>,
}

impl RequestBuilder {
    pub(crate) fn new(client: Client, method: Method, url: &str) -> Self {
        let request = Url::parse(url)
            .map_err(|error| Error::new(Kind::Builder, error.to_string()))
            .and_then(|mut url| {
                let mut headers = client.settings.headers.clone();
                remove_url_credentials(&mut url, &mut headers)?;
                Ok(Request { method, url, headers, body: None, timeout: None })
            });
        Self { client, request }
    }
    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        HeaderValue: TryFrom<V>,
    {
        if let Ok(request) = &mut self.request {
            match (HeaderName::try_from(key), HeaderValue::try_from(value)) {
                (Ok(key), Ok(value)) => {
                    request.headers.insert(key, value);
                }
                _ => {
                    self.request = Err(Error::new(Kind::Builder, "Invalid HTTP header"));
                }
            }
        }
        self
    }
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        if let Ok(request) = &mut self.request {
            request.headers.extend(headers);
        }
        self
    }
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        if let Ok(request) = &mut self.request {
            request.body = Some(body.into());
        }
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        if let Ok(request) = &mut self.request {
            request.timeout = Some(timeout);
        }
        self
    }
    pub fn json<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => {
                self = self.body(body).header(CONTENT_TYPE, "application/json");
            }
            Err(error) => {
                self.request = Err(Error::new(Kind::Builder, error.to_string()));
            }
        }
        self
    }
    pub fn bearer_auth(self, token: impl std::fmt::Display) -> Self {
        self.header(AUTHORIZATION, format!("Bearer {token}"))
    }
    pub fn basic_auth<U, P>(self, username: U, password: Option<P>) -> Self
    where
        U: std::fmt::Display,
        P: std::fmt::Display,
    {
        let credentials = format!(
            "{username}:{}",
            password.map(|password| password.to_string()).unwrap_or_default()
        );
        self.header(AUTHORIZATION, format!("Basic {}", STANDARD.encode(credentials)))
    }
    #[must_use]
    pub fn try_clone(&self) -> Option<Self> {
        Some(Self { client: self.client.clone(), request: Ok(self.request.as_ref().ok()?.clone()) })
    }
    pub fn build(self) -> Result<Request> {
        self.request
    }
    pub async fn send(self) -> Result<Response> {
        self.request?.send(&self.client).await
    }
}

impl Request {
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }
    #[must_use]
    pub fn method(&self) -> &Method {
        &self.method
    }
    #[must_use]
    pub fn try_clone(&self) -> Option<Self> {
        Some(self.clone())
    }
    pub(crate) async fn send(mut self, client: &Client) -> Result<Response> {
        let mut previous = Vec::new();
        let started = Instant::now();
        let total_timeout = self.timeout.or(client.settings.total_timeout);
        loop {
            self.timeout = remaining_timeout(total_timeout, started)?;
            let response = self.send_once(client).await?;
            if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                return Ok(response);
            }
            let Some(location) = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
            else {
                return Ok(response);
            };
            let target = self.url
                .join(location)
                .map_err(|error| Error::new(Kind::Redirect, error.to_string()))?;
            previous.push(self.url.clone());
            match client.settings.redirect.apply(&target, &previous) {
                Action::Stop => return Ok(response),
                Action::Error(error) => return Err(error),
                Action::Follow => self.follow_redirect(target, response.status().as_u16())?,
            }
        }
    }
    async fn send_once(&self, client: &Client) -> Result<Response> {
        if !matches!(self.url.scheme(), "http" | "https") {
            return Err(Error::new(Kind::Builder, "Unsupported HTTP URL scheme"));
        }
        let mut headers: Vec<_> = self.headers
            .iter()
            .map(|(name, value)| {
                value
                    .to_str()
                    .map(|value| (name.as_str(), value))
                    .map_err(|error| Error::new(Kind::Builder, error.to_string()))
            })
            .collect::<Result<_>>()?;
        let body_length = self.body
            .as_ref()
            .map(|body| body.len().to_string());
        if let Some(length) = &body_length
            && !self.headers.contains_key(CONTENT_LENGTH)
        {
            headers.push(("content-length", length));
        }
        let result = crate::upload::send(json!({
            "operation": "network.request", "url": self.url.as_str(), "method": self.method.as_str(),
            "headers": headers,
            "timeoutMs": client.settings.timeout.as_millis(),
            "totalTimeoutMs": self.timeout.or(client.settings.total_timeout).map(|timeout| timeout.as_millis().max(1)),
        }), self.body.as_deref()).await?;
        Response::from_host(result)
    }
    fn follow_redirect(&mut self, mut target: Url, status: u16) -> Result<()> {
        if !matches!(target.scheme(), "http" | "https") {
            return Err(Error::new(Kind::Redirect, "Unsupported redirect scheme"));
        }
        if status == 303 && self.method != Method::HEAD
            || matches!(status, 301 | 302) && self.method == Method::POST
        {
            self.method = Method::GET;
            self.body = None;
            self.headers.remove(CONTENT_TYPE);
            self.headers.remove(CONTENT_LENGTH);
        }
        if self.url.origin() != target.origin() {
            if self.body.is_some() {
                return Err(Error::new(
                    Kind::Redirect,
                    "Cannot replay request body across origins",
                ));
            }
            for name in [
                AUTHORIZATION,
                COOKIE,
                PROXY_AUTHORIZATION,
                WWW_AUTHENTICATE,
                HeaderName::from_static("cookie2"),
                HeaderName::from_static("npm-otp"),
            ] {
                self.headers.remove(name);
            }
        }
        target.set_username("").map_err(|()| Error::new(Kind::Redirect, "Invalid redirect URL"))?;
        target
            .set_password(None)
            .map_err(|()| Error::new(Kind::Redirect, "Invalid redirect URL"))?;
        self.url = target;
        Ok(())
    }
}

fn remaining_timeout(timeout: Option<Duration>, started: Instant) -> Result<Option<Duration>> {
    timeout
        .map(|timeout| {
            timeout
                .checked_sub(started.elapsed())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| Error::new(Kind::Timeout, "HTTP request deadline timed out"))
        })
        .transpose()
}

fn remove_url_credentials(url: &mut Url, headers: &mut HeaderMap) -> Result<()> {
    if url.username().is_empty() && url.password().is_none() {
        return Ok(());
    }
    let invalid = || Error::new(Kind::Builder, "Invalid credentials in HTTP URL");
    let username = percent_encoding::percent_decode_str(url.username())
        .decode_utf8()
        .map_err(|_| invalid())?;
    let password = percent_encoding::percent_decode_str(url.password().unwrap_or_default())
        .decode_utf8()
        .map_err(|_| invalid())?;
    let value = format!("Basic {}", STANDARD.encode(format!("{username}:{password}")));
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&value).map_err(|_| invalid())?);
    url.set_username("").map_err(|()| invalid())?;
    url.set_password(None).map_err(|()| invalid())?;
    Ok(())
}

#[cfg(test)]
mod tests;
