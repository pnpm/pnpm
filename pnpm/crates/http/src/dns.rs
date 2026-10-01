use std::{future::Future, net::SocketAddr, pin::Pin, str::FromStr};

pub type Addrs = Box<dyn Iterator<Item = SocketAddr> + Send>;
pub type Resolving =
    Pin<Box<dyn Future<Output = Result<Addrs, Box<dyn std::error::Error + Send + Sync>>> + Send>>;

#[derive(Clone, Debug)]
pub struct Name(String);
impl Name {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for Name {
    type Err = std::io::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}

pub trait Resolve: Send + Sync {
    fn resolve(&self, name: Name) -> Resolving;
    fn uses_host_resolution(&self) -> bool {
        false
    }
}
