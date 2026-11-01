//! Matching platforms against a host, and selecting the variant that fits it
//! best.

use super::{Floor, Libc, NodeAbi, Platform};
use derive_more::{Display, Error};
use std::cmp::Ordering;

/// What a client knows about the host it selects a variant for.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Host {
    pub os: String,
    pub cpu: String,
    /// `None` outside Linux; on Linux, `None` means glibc.
    pub libc: Option<Libc>,
    /// `None` for the running host, whose versions are probed the first
    /// time a platform with a floor or ABI is matched against it.
    pub floors: Option<HostFloors>,
}

impl Host {
    fn effective_libc(&self) -> Option<Libc> {
        match self.libc {
            None if self.os == "linux" => Some(Libc::Glibc),
            libc => libc,
        }
    }
}

/// The host's versions that floors and ABIs are matched against. A value
/// the client does not know is `None`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostFloors {
    pub libc_version: Option<Floor>,
    pub os_version: Option<Floor>,
    pub napi: Option<u32>,
    /// The ABI of the runtime addons load into.
    pub node_abi: Option<NodeAbi>,
}

/// Whether a host meets `floor`. A floor the host's value is unknown for
/// matches, so a platform listed in `supportedArchitectures` without
/// versions fetches its variants.
fn meets<Value: Ord>(floor: Option<Value>, host: Option<Value>) -> bool {
    match (floor, host) {
        (Some(floor), Some(host)) => host >= floor,
        _ => true,
    }
}

impl Platform {
    /// The number of fields a host matches when it matches this platform.
    fn field_count(&self) -> usize {
        if self.wasm32 {
            return 1;
        }
        let libc = self.libc.is_some() || self.os.as_deref() == Some("linux");
        2 + usize::from(libc)
            + usize::from(self.libc_version.is_some())
            + usize::from(self.os_version.is_some())
            + usize::from(self.napi.is_some())
            + usize::from(self.node_abi.is_some())
    }

    /// Whether `host` runs this platform. An unknown ABI does not match,
    /// since loading the wrong one fails.
    #[must_use]
    pub fn matches(&self, host: &Host) -> bool {
        if self.wasm32 {
            return true;
        }
        self.os.as_deref() == Some(host.os.as_str())
            && self.cpu.as_deref() == Some(host.cpu.as_str())
            && self.effective_libc() == host.effective_libc()
            && (!self.has_floor()
                || self.meets_floors(&host.floors.unwrap_or_else(super::host_floors::detect)))
    }

    fn has_floor(&self) -> bool {
        self.libc_version.is_some()
            || self.os_version.is_some()
            || self.napi.is_some()
            || self.node_abi.is_some()
    }

    fn meets_floors(&self, floors: &HostFloors) -> bool {
        meets(self.libc_version, floors.libc_version)
            && meets(self.os_version, floors.os_version)
            && meets(self.napi, floors.napi)
            && self.node_abi.is_none_or(|abi| floors.node_abi == Some(abi))
    }

    /// How well this platform fits a host it matches: more matched fields,
    /// then the highest glibc, OS, and N-API floors. `wasm32` ranks last.
    fn rank(&self) -> Rank {
        Rank {
            native: !self.wasm32,
            fields: self.field_count(),
            libc_version: self.libc_version,
            os_version: self.os_version,
            napi: self.napi,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    native: bool,
    fields: usize,
    libc_version: Option<Floor>,
    os_version: Option<Floor>,
    napi: Option<u32>,
}

/// Two variants fit a host equally well. Publication rejects versions where
/// this can happen, so a client fails rather than pick one.
#[derive(Debug, Display, Error, Clone, Copy, PartialEq, Eq)]
#[display("variants {first} and {second} fit the host equally well")]
pub struct AmbiguousSelection {
    pub first: usize,
    pub second: usize,
}

/// The index of the variant that fits `host` best, comparing each variant by
/// its best matching platform, or `None` when no variant matches.
pub fn select<'platforms, Variants>(
    variants: Variants,
    host: &Host,
) -> Result<Option<usize>, AmbiguousSelection>
where
    Variants: IntoIterator<Item = &'platforms [Platform]>,
{
    let mut best: Option<(Rank, usize)> = None;
    let mut tie: Option<usize> = None;
    for (index, platforms) in variants.into_iter().enumerate() {
        let Some(rank) = platforms
            .iter()
            .filter(|platform| platform.matches(host))
            .map(Platform::rank)
            .max()
        else {
            continue;
        };
        match best.map(|(best, _)| rank.cmp(&best)) {
            None | Some(Ordering::Greater) => {
                best = Some((rank, index));
                tie = None;
            }
            Some(Ordering::Equal) => tie = Some(index),
            Some(Ordering::Less) => {}
        }
    }
    match (best, tie) {
        (Some((_, first)), Some(second)) => Err(AmbiguousSelection { first, second }),
        (best, _) => Ok(best.map(|(_, index)| index)),
    }
}

/// The variants' order in the lockfile: native before `wasm32`, then by the
/// rank of each variant's best platform, highest first.
#[must_use]
pub fn selection_order(variants: &[&[Platform]]) -> Vec<usize> {
    let best = |platforms: &[Platform]| {
        platforms
            .iter()
            .map(Platform::rank)
            .max()
    };
    let mut order: Vec<usize> = (0..variants.len()).collect();
    order.sort_by(|&a, &b| best(variants[b]).cmp(&best(variants[a])));
    order
}
