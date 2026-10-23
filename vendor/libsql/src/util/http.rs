use super::box_clone_service::BoxCloneService;
use tokio::io::{AsyncRead, AsyncWrite};

pub trait Socket:
    hyper::client::connect::Connection + AsyncRead + AsyncWrite + Send + Unpin + 'static + Sync
{
}

impl<T> Socket for T where
    T: hyper::client::connect::Connection + AsyncRead + AsyncWrite + Send + Unpin + 'static + Sync
{
}

impl hyper::client::connect::Connection for Box<dyn Socket> {
    fn connected(&self) -> hyper::client::connect::Connected {
        self.as_ref().connected()
    }
}

pub type ConnectorService =
    BoxCloneService<http::Uri, Box<dyn Socket>, Box<dyn std::error::Error + Sync + Send + 'static>>;

#[cfg(feature = "replication")]
pub type HttpRequestCallback = std::sync::Arc<dyn Fn(&mut http::Request<()>) + Send + Sync>;
