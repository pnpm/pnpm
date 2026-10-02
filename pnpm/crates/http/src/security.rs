use crate::{Error, Result, Url, error::Kind};
use rustls::pki_types::{CertificateDer, pem::PemObject};

#[derive(Clone, Debug)]
pub struct Certificate(Vec<u8>);
impl Certificate {
    pub fn from_der(bytes: &[u8]) -> Result<Self> {
        Ok(Self(bytes.to_vec()))
    }
    pub fn from_pem(bytes: &[u8]) -> Result<Self> {
        let certificate = CertificateDer::from_pem_slice(bytes)
            .map_err(|error| Error::new(Kind::Builder, error.to_string()))?;
        Ok(Self(certificate.to_vec()))
    }
    pub fn from_pem_bundle(bytes: &[u8]) -> Result<Vec<Self>> {
        CertificateDer::pem_slice_iter(bytes)
            .map(|certificate| {
                certificate
                    .map(|certificate| Self(certificate.to_vec()))
                    .map_err(|error| Error::new(Kind::Builder, error.to_string()))
            })
            .collect()
    }
    pub(crate) fn into_der(self) -> Vec<u8> {
        self.0
    }
}
#[derive(Clone, Debug)]
pub struct Identity;
impl Identity {
    pub fn from_pem(_bytes: &[u8]) -> Result<Self> {
        Err(Error::new(Kind::Builder, "Client TLS identities are unavailable in WebContainers"))
    }
}
#[derive(Clone, Debug)]
pub struct Proxy;
impl Proxy {
    pub fn custom<F>(_callback: F) -> Self
    where
        F: Fn(&Url) -> Option<Url> + Send + Sync + 'static,
    {
        Self
    }
    #[must_use]
    pub fn basic_auth(self, _username: &str, _password: &str) -> Self {
        self
    }
}
