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

impl IpNetwork {
    fn range(self) -> AddressRange {
        let (family, address, width) = match self.address {
            IpAddr::V4(address) => (Family::V4, u128::from(u32::from(address)), 32),
            IpAddr::V6(address) => (Family::V6, u128::from(address), 128),
        };
        let host_bits = u32::from(width - self.prefix);
        let host_mask = u128::MAX
            .checked_shr(128 - host_bits)
            .unwrap_or(0);
        AddressRange { family, first: address & !host_mask, last: address | host_mask }
    }
}

/// The addresses that at least one of several networks holds.
#[derive(Debug, Default, Clone)]
pub struct IpNetworkSet {
    /// Disjoint, non-adjacent ranges, sorted by family and then first address.
    ranges: Vec<AddressRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    V4,
    V6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct AddressRange {
    family: Family,
    first: u128,
    last: u128,
}

impl IpNetworkSet {
    pub fn new(networks: impl IntoIterator<Item = IpNetwork>) -> Self {
        let mut ranges: Vec<AddressRange> = networks
            .into_iter()
            .map(IpNetwork::range)
            .collect();
        ranges.sort_unstable();
        let mut merged: Vec<AddressRange> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                Some(last)
                    if last.family == range.family
                        && range.first <= last.last.saturating_add(1) =>
                {
                    last.last = last.last.max(range.last);
                }
                _ => merged.push(range),
            }
        }
        Self { ranges: merged }
    }

    /// Whether every address of `network` is in the set, possibly spread
    /// over several of its networks.
    #[must_use]
    pub fn covers(&self, network: &IpNetwork) -> bool {
        let wanted = network.range();
        let after = self.ranges.partition_point(|range| {
            (range.family, range.first) <= (wanted.family, wanted.first)
        });
        after
            .checked_sub(1)
            .and_then(|index| self.ranges.get(index))
            .is_some_and(|range| range.family == wanted.family && range.last >= wanted.last)
    }
}

/// Parse a list of trusted-proxy entries, which may include CIDR networks, bare
/// IP addresses, or the `"loopback"` keyword (which expands to `127.0.0.0/8` and `::1/128`).
pub fn parse_trusted_proxies(
    entries: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<IpNetwork>, RegistryError> {
    let mut networks = Vec::new();
    for entry in entries {
        let raw = entry.as_ref().trim();
        if raw.is_empty() {
            continue;
        }
        if raw.eq_ignore_ascii_case("loopback") {
            networks.push(IpNetwork::parse("127.0.0.0/8")?);
            networks.push(IpNetwork::parse("::1/128")?);
        } else {
            networks.push(IpNetwork::parse(raw)?);
        }
    }
    Ok(networks)
}

fn address_width(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

#[cfg(test)]
mod tests;
