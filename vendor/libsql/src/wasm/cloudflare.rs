use crate::hrana::{HranaError, HttpBody, HttpSend, Result};
use bytes::Bytes;
use futures::{ready, Stream};
use std::future::Future;
use std::io::ErrorKind;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use worker::wasm_bindgen::JsValue;

#[derive(Debug, Copy, Clone)]
pub struct CloudflareSender(());

impl CloudflareSender {
    pub(crate) fn new() -> Self {
        CloudflareSender(())
    }

    async fn send(url: Arc<str>, auth: Arc<str>, body: String) -> Result<HttpBody<HttpStream>> {
        use worker::{
            CfProperties, Fetch, Headers, Method, Request, RequestInit, RequestRedirect,
            ResponseBody,
        };

        let mut response = Fetch::Request(Request::new_with_init(
            url.as_ref(),
            &RequestInit {
                body: Some(JsValue::from(body)),
                headers: {
                    let headers = Headers::new();
                    headers.append("Authorization", auth.as_ref())?;
                    headers
                },
                cf: CfProperties::new(),
                method: Method::Post,
                redirect: RequestRedirect::Follow,
            },
        )?)
        .send()
        .await?;
        if response.status_code() != 200 {
            let body = response.text().await?;
            Err(HranaError::Api(body))
        } else {
            let body: HttpBody<HttpStream> = match response.body() {
                ResponseBody::Empty => HttpBody::from(Bytes::new()),
                ResponseBody::Body(body) => HttpBody::from(Bytes::from(body.clone())),
                _ => HttpBody::Stream(HttpStream(response.stream()?)),
            };
            Ok(body)
        }
    }
}

impl HttpSend for CloudflareSender {
    type Stream = HttpBody<HttpStream>;
    type Result = Pin<Box<dyn Future<Output = Result<Self::Stream>>>>;

    fn http_send(&self, url: Arc<str>, auth: Arc<str>, body: String) -> Self::Result {
        let fut = Self::send(url, auth, body);
        Box::pin(fut)
    }

    fn oneshot(self, url: Arc<str>, auth: Arc<str>, body: String) {
        worker::wasm_bindgen_futures::spawn_local(async move {
            let _ = Self::send(url, auth, body).await;
        });
    }
}

impl From<worker::Error> for HranaError {
    fn from(value: worker::Error) -> Self {
        // This converts it to a string due to the error type being !Send/!Sync which will break
        // a lot of stuff.
        HranaError::Http(value.to_string())
    }
}

pub struct HttpStream(worker::ByteStream);

impl Stream for HttpStream {
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let pin = Pin::new(&mut self.0);
        let res = ready!(pin.poll_next(cx));
        match res {
            None => Poll::Ready(None),
            Some(Ok(data)) => Poll::Ready(Some(Ok(Bytes::from(data)))),
            Some(Err(e)) => Poll::Ready(Some(Err(std::io::Error::new(
                ErrorKind::Other,
                e.to_string(),
            )))),
        }
    }
}
