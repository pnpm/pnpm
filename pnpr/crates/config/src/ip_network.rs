use pnpr_error::RegistryError;
use std::net::IpAddr;

/// An IP network in CIDR notation, such as `10.0.0.0/8` or `fd00::/8`. A bare
/// address is the network holding only that address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpNetwork {
    address: IpAddr,
    prefix: u8,
}

impl IpNetwork {
    pub fn parse(raw: &str) -> Result<Self, RegistryError> {
        let invalid = || RegistryError::InvalidConfig {
            reason: format!("{raw:?} is not an IP address or a CIDR network"),
        };
        let (address, prefix) = raw
            .split_once('/')
            .map_or((raw, None), |(address, prefix)| (address, Some(prefix)));
        let address: IpAddr = address.parse().map_err(|_| invalid())?;
        let address = address.to_canonical();
        let width = address_width(address);
        let prefix = match prefix {
            None => width,
            Some(prefix) => prefix
                .parse::<u8>()
                .ok()
                .filter(|prefix| *prefix <= width)
                .ok_or_else(invalid)?,
        };
        Ok(Self { address, prefix })
    }

    /// An IPv4-mapped IPv6 address belongs to the network of its IPv4 form.
    #[must_use]
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.address, address.to_canonical()) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

fn address_width(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

#[cfg(test)]
mod tests;
