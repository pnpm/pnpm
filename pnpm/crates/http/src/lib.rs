#[cfg(target_family = "wasm")]
pub mod dns;
#[cfg(target_family = "wasm")]
pub mod redirect;

#[cfg(target_family = "wasm")]
pub use client::{Client, ClientBuilder};
#[cfg(target_family = "wasm")]
pub use error::Error;
#[cfg(target_family = "wasm")]
pub use http::{Method, StatusCode, header};
#[cfg(target_family = "wasm")]
pub use request::{Request, RequestBuilder};
#[cfg(not(target_family = "wasm"))]
pub use reqwest::*;
#[cfg(target_family = "wasm")]
pub use response::Response;
#[cfg(target_family = "wasm")]
pub use security::{Certificate, Identity, Proxy};
#[cfg(target_family = "wasm")]
pub use url::Url;

#[cfg(target_family = "wasm")]
mod client;
#[cfg(target_family = "wasm")]
mod error;
#[cfg(target_family = "wasm")]
mod request;
#[cfg(target_family = "wasm")]
mod response;
#[cfg(target_family = "wasm")]
mod security;
#[cfg(target_family = "wasm")]
mod upload;

#[cfg(target_family = "wasm")]
pub type Result<T> = std::result::Result<T, Error>;
