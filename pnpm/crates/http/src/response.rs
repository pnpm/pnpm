use crate::{
    Error, Result, StatusCode, Url,
    error::Kind,
    header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue},
};
use bytes::Bytes;
use futures_util::{Stream, stream};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::pin::Pin;

#[derive(Debug)]
pub struct Response {
    handle: u32,
    status: StatusCode,
    headers: HeaderMap,
    url: Url,
}
impl Response {
    pub(crate) fn from_host(value: Value) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Metadata {
            handle: u32,
            status: u16,
            headers: Vec<(String, String)>,
            url: String,
        }
        let metadata: Metadata = serde_json::from_value(value)
            .map_err(|error| Error::new(Kind::Decode, error.to_string()))?;
        let mut headers = HeaderMap::new();
        for (name, value) in metadata.headers {
            headers.append(
                HeaderName::try_from(name)
                    .map_err(|error| Error::new(Kind::Decode, error.to_string()))?,
                HeaderValue::try_from(value)
                    .map_err(|error| Error::new(Kind::Decode, error.to_string()))?,
            );
        }
        Ok(Self {
            handle: metadata.handle,
            status: StatusCode::from_u16(metadata.status)
                .map_err(|error| Error::new(Kind::Decode, error.to_string()))?,
            url: Url::parse(&metadata.url)
                .map_err(|error| Error::new(Kind::Decode, error.to_string()))?,
            headers,
        })
    }
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }
    pub fn content_length(&self) -> Option<u64> {
        self.headers
            .get(CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }
    pub fn error_for_status(self) -> Result<Self> {
        self.error_for_status_ref()?;
        Ok(self)
    }
    pub fn error_for_status_ref(&self) -> Result<&Self> {
        if self.status.is_client_error() || self.status.is_server_error() {
            return Err(Error::status_error(self.status, self.url.clone()));
        }
        Ok(self)
    }
    pub async fn chunk(&mut self) -> Result<Option<Bytes>> {
        let value =
            pnpm_wasm_host::request(&json!({"operation":"stream.read", "handle":self.handle}))
                .map_err(Error::body)?
                .await
                .map_err(Error::body)?;
        #[derive(serde::Deserialize)]
        struct Chunk {
            done: bool,
            bytes: Vec<u8>,
        }
        let chunk: Chunk = serde_json::from_value(value)
            .map_err(|error| Error::new(Kind::Decode, error.to_string()))?;
        Ok((!chunk.done).then(|| Bytes::from(chunk.bytes)))
    }
    pub async fn bytes(mut self) -> Result<Bytes> {
        let mut bytes = Vec::new();
        while let Some(chunk) = self.chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        Ok(Bytes::from(bytes))
    }
    pub async fn text(self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes().await?).into_owned())
    }
    pub async fn json<T: DeserializeOwned>(self) -> Result<T> {
        serde_json::from_slice(&self.bytes().await?)
            .map_err(|error| Error::new(Kind::Decode, error.to_string()))
    }
    pub fn bytes_stream(self) -> Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>> {
        Box::pin(stream::try_unfold(self, |mut response| async move {
            response
                .chunk()
                .await
                .map(|chunk| chunk.map(|chunk| (chunk, response)))
        }))
    }
}
impl Drop for Response {
    fn drop(&mut self) {
        if let Err(error) = pnpm_wasm_host::close_resource(self.handle) {
            eprintln!("Failed to close HTTP response stream: {error}");
        }
    }
}
