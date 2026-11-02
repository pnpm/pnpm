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

/// Why a platform is matched against a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Choosing the archive to link: a floor or ABI whose host value is
    /// unknown is not met, since linking an archive the host may not run is
    /// worse than linking the primary.
    Link,
    /// Choosing the archives to fetch for a platform other than the host,
    /// such as one named in `supportedArchitectures`: a floor or ABI whose
    /// value that platform does not state is met, since an extra archive only
    /// costs disk and a missing one makes an offline install fail.
    Fetch,
}

/// Whether a host meets `floor`, given whether an unknown host value counts.
fn meets<Value: Ord>(floor: Option<Value>, host: Option<Value>, purpose: Purpose) -> bool {
    match (floor, host) {
        (None, _) => true,
        (Some(floor), Some(host)) => host >= floor,
        (Some(_), None) => purpose == Purpose::Fetch,
    }
}

impl Platform {
    /// The number of explicit constraints a host matches when it matches
    /// this platform: `os`, `cpu`, Linux `libc` (glibc included), each floor,
    /// and the ABI constraint once.
    fn constraint_count(&self) -> usize {
        if self.wasm32 {
            return 1;
        }
        let libc = self.libc.is_some() || self.os.as_deref() == Some("linux");
        2 + usize::from(libc)
            + usize::from(self.libc_version.is_some())
            + usize::from(self.os_version.is_some())
            + usize::from(self.napi.is_some() || self.node_abi.is_some())
    }

    /// Whether `host` runs this platform, for choosing the archive to link.
    #[must_use]
    pub fn matches(&self, host: &Host) -> bool {
        self.matches_for(host, Purpose::Link)
    }

    /// Whether this platform matches `host` for `purpose`.
    #[must_use]
    pub fn matches_for(&self, host: &Host, purpose: Purpose) -> bool {
        if self.wasm32 {
            return true;
        }
        self.os.as_deref() == Some(host.os.as_str())
            && self.cpu.as_deref() == Some(host.cpu.as_str())
            && self.effective_libc() == host.effective_libc()
            && (!self.has_floor()
                || self.meets_floors(
                    &host.floors.unwrap_or_else(super::host_floors::detect),
                    purpose,
                ))
    }

    fn has_floor(&self) -> bool {
        self.libc_version.is_some()
            || self.os_version.is_some()
            || self.napi.is_some()
            || self.node_abi.is_some()
    }

    fn meets_floors(&self, floors: &HostFloors, purpose: Purpose) -> bool {
        meets(self.libc_version, floors.libc_version, purpose)
            && meets(self.os_version, floors.os_version, purpose)
            && meets(self.napi, floors.napi, purpose)
            && self.node_abi.is_none_or(|abi| match floors.node_abi {
                Some(host) => host == abi,
                None => purpose == Purpose::Fetch,
            })
    }

    /// How well this platform fits a host it matches: native above `wasm32`,
    /// then more explicit constraints, then the highest glibc and OS floors,
    /// then an exact ABI above an N-API floor above no ABI constraint, and the
    /// highest N-API floor.
    fn rank(&self) -> Rank {
        let abi = match (self.node_abi, self.napi) {
            (Some(_), _) => (2, 0),
            (None, Some(napi)) => (1, napi),
            (None, None) => (0, 0),
        };
        Rank {
            native: !self.wasm32,
            constraints: self.constraint_count(),
            libc_version: self.libc_version,
            os_version: self.os_version,
            abi,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    native: bool,
    constraints: usize,
    libc_version: Option<Floor>,
    os_version: Option<Floor>,
    /// An exact `nodeAbi` (2), an N-API floor (1, with the floor), or none.
    abi: (u8, u32),
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
