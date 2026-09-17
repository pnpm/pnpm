//! One platform `supportedArchitectures` names.

use derive_more::{Display, Error};
use miette::Diagnostic;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

/// One platform an install prepares for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportedPlatform {
    /// `current`: the platform the install runs on.
    Current,
    Named(NamedPlatform),
}

/// A platform named outright, as `<os>-<cpu>[-<libc>]` or as the Rust
/// target triple of the same machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedPlatform {
    pub os: Os,
    pub architecture: Architecture,
    /// The C library its packages are built against, absent on a system
    /// that has only one. Linux names one either way: a Linux platform
    /// that leaves it out is the glibc platform, the way `linux-x64`
    /// names the glibc build everywhere else in pnpm.
    pub libc: Option<Libc>,
}

/// The systems a platform can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Darwin,
    Windows,
}

/// A Linux C library, with the oldest release of it a package may be
/// built against when the platform names one. Only Python wheels carry
/// that baseline, in their `manylinux` and `musllinux` tags; a package
/// that declares `libc` names the family alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Libc {
    pub family: LibcFamily,
    /// The `manylinux_2_28` or `musllinux_1_1` baseline as written.
    pub baseline: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibcFamily {
    Glibc,
    Musl,
}

/// Why a `supportedArchitectures` entry does not name a platform.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnknownPlatformError {
    #[display("pnpm does not know the platform {platform}")]
    #[diagnostic(
        code(ERR_PNPM_UNKNOWN_PLATFORM),
        help(
            "Name a platform as <os>-<cpu>, such as linux-x64, darwin-arm64 or win32-x64. A Linux platform may name its C library too, as linux-x64-musl or linux-x64-manylinux_2_28."
        )
    )]
    Unknown {
        #[error(not(source))]
        platform: String,
    },

    #[display("only a Linux platform names a C library, and {platform} is not one")]
    #[diagnostic(code(ERR_PNPM_LIBC_ON_NON_LINUX_PLATFORM))]
    LibcOnNonLinux {
        #[error(not(source))]
        platform: String,
    },
}

impl Os {
    /// The name packages declare under `os`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Darwin => "darwin",
            Self::Windows => "win32",
        }
    }

    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "linux" => Some(Self::Linux),
            "darwin" => Some(Self::Darwin),
            "win32" => Some(Self::Windows),
            _ => None,
        }
    }
}

impl LibcFamily {
    /// The name packages declare under `libc`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Glibc => "glibc",
            Self::Musl => "musl",
        }
    }
}

impl Libc {
    /// The C library alone, as a platform that names no wheel baseline
    /// asks for it.
    #[must_use]
    pub fn family(family: LibcFamily) -> Self {
        Self { family, baseline: None }
    }

    /// The name packages declare under `libc`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.family.name()
    }

    pub(super) fn parse(token: &str) -> Option<Self> {
        match token {
            "gnu" | "glibc" => Some(Self::family(LibcFamily::Glibc)),
            "musl" => Some(Self::family(LibcFamily::Musl)),
            baseline => Some(Self {
                family: baseline_family(baseline)?,
                baseline: Some(baseline.to_string()),
            }),
        }
    }
}

/// The C library a Python wheel baseline is built against.
fn baseline_family(token: &str) -> Option<LibcFamily> {
    if token.starts_with("manylinux_") {
        Some(LibcFamily::Glibc)
    } else if token.starts_with("musllinux_") {
        Some(LibcFamily::Musl)
    } else {
        None
    }
}

/// How one architecture is spelled, since the three worlds a platform is
/// read in disagree: `platform` names it inside a platform, `cpu` is what
/// packages declare, and `wheel` is what a Python wheel tag carries.
/// `aliases` accepts the rest, which is how the Rust target triple of the
/// same machine parses.
struct ArchitectureNames {
    architecture: Architecture,
    platform: &'static str,
    cpu: &'static str,
    wheel: &'static str,
    aliases: &'static [&'static str],
}

/// Every architecture a platform can name.
const ARCHITECTURES: &[ArchitectureNames] = &[
    ArchitectureNames {
        architecture: Architecture::X64,
        platform: "x64",
        cpu: "x64",
        wheel: "x86_64",
        aliases: &["amd64"],
    },
    ArchitectureNames {
        architecture: Architecture::Arm64,
        platform: "arm64",
        cpu: "arm64",
        wheel: "aarch64",
        aliases: &[],
    },
    ArchitectureNames {
        architecture: Architecture::Ia32,
        platform: "ia32",
        cpu: "ia32",
        wheel: "i686",
        aliases: &["x86"],
    },
    ArchitectureNames {
        architecture: Architecture::Arm,
        platform: "arm",
        cpu: "arm",
        wheel: "armv7l",
        aliases: &["armv7"],
    },
    ArchitectureNames {
        architecture: Architecture::Ppc64,
        platform: "ppc64",
        cpu: "ppc64",
        wheel: "ppc64",
        aliases: &["powerpc64"],
    },
    ArchitectureNames {
        architecture: Architecture::Ppc64Le,
        platform: "ppc64le",
        cpu: "ppc64",
        wheel: "ppc64le",
        aliases: &["powerpc64le"],
    },
    ArchitectureNames {
        architecture: Architecture::S390x,
        platform: "s390x",
        cpu: "s390x",
        wheel: "s390x",
        aliases: &[],
    },
    ArchitectureNames {
        architecture: Architecture::Riscv64,
        platform: "riscv64",
        cpu: "riscv64",
        wheel: "riscv64",
        aliases: &["riscv64gc"],
    },
];

/// The architectures a platform can name. Each is spelled three ways,
/// which [`Architecture::cpu`] and [`Architecture::wheel`] reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X64,
    Arm64,
    Ia32,
    Arm,
    /// Big-endian, which Node reports as `ppc64` the same way it reports
    /// [`Self::Ppc64Le`]: the two are one name to a package and two
    /// platforms to a wheel.
    Ppc64,
    Ppc64Le,
    S390x,
    Riscv64,
}

impl Architecture {
    /// The name packages declare under `cpu`.
    #[must_use]
    pub fn cpu(self) -> &'static str {
        self.names().cpu
    }

    /// The name a Python wheel tag carries.
    #[must_use]
    pub fn wheel(self) -> &'static str {
        self.names().wheel
    }

    fn platform_name(self) -> &'static str {
        self.names().platform
    }

    fn names(self) -> &'static ArchitectureNames {
        ARCHITECTURES
            .iter()
            .find(|names| names.architecture == self)
            .expect("every architecture is in ARCHITECTURES")
    }

    pub(super) fn parse(name: &str) -> Option<Self> {
        ARCHITECTURES
            .iter()
            .find(|names| {
                names.platform == name
                    || names.cpu == name
                    || names.wheel == name
                    || names.aliases.contains(&name)
            })
            .map(|names| names.architecture)
    }
}

impl fmt::Display for SupportedPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Current => formatter.write_str("current"),
            Self::Named(platform) => platform.fmt(formatter),
        }
    }
}

impl fmt::Display for NamedPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}", self.os.name(), self.architecture.platform_name())?;
        match &self.libc {
            Some(Libc { baseline: Some(baseline), .. }) => write!(formatter, "-{baseline}"),
            Some(Libc { family: LibcFamily::Musl, .. }) => formatter.write_str("-musl"),
            _ => Ok(()),
        }
    }
}

impl FromStr for SupportedPlatform {
    type Err = UnknownPlatformError;

    fn from_str(entry: &str) -> Result<Self, Self::Err> {
        if entry == "current" {
            return Ok(Self::Current);
        }
        parse_named(entry).map(Self::Named)
    }
}

fn parse_named(entry: &str) -> Result<NamedPlatform, UnknownPlatformError> {
    let (head, rest) = entry.split_once('-').ok_or_else(|| unknown(entry))?;
    match Os::parse(head) {
        Some(os) => parse_platform_name(entry, os, rest),
        None => parse_target_triple(entry, head, rest),
    }
}

/// `<os>-<cpu>[-<libc>]`, the spelling `pnpm pack-app --target` and the
/// `supportedArchitectures` axes share.
fn parse_platform_name(
    entry: &str,
    os: Os,
    rest: &str,
) -> Result<NamedPlatform, UnknownPlatformError> {
    let (architecture, libc) = match rest.split_once('-') {
        Some((architecture, libc)) => (architecture, Some(libc)),
        None => (rest, None),
    };
    let architecture = Architecture::parse(architecture).ok_or_else(|| unknown(entry))?;
    let libc = match (os, libc) {
        (Os::Linux, Some(libc)) => Some(Libc::parse(libc).ok_or_else(|| unknown(entry))?),
        (Os::Linux, None) => Some(Libc::family(LibcFamily::Glibc)),
        (_, None) => None,
        (_, Some(_)) => {
            return Err(UnknownPlatformError::LibcOnNonLinux { platform: entry.to_string() });
        }
    };
    Ok(NamedPlatform { os, architecture, libc })
}

/// The Rust target triple of a machine, which is how uv and the rest of
/// the Python tooling name the same platform.
fn parse_target_triple(
    entry: &str,
    architecture: &str,
    system: &str,
) -> Result<NamedPlatform, UnknownPlatformError> {
    let architecture = Architecture::parse(architecture).ok_or_else(|| unknown(entry))?;
    let (os, libc) = match system {
        "apple-darwin" => (Os::Darwin, None),
        "pc-windows-msvc" => (Os::Windows, None),
        _ => (
            Os::Linux,
            Some(
                Libc::parse(system.strip_prefix("unknown-linux-").unwrap_or(system))
                    .ok_or_else(|| unknown(entry))?,
            ),
        ),
    };
    Ok(NamedPlatform { os, architecture, libc })
}

fn unknown(entry: &str) -> UnknownPlatformError {
    UnknownPlatformError::Unknown { platform: entry.to_string() }
}

impl Serialize for SupportedPlatform {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SupportedPlatform {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        let entry = String::deserialize(deserializer)?;
        entry.parse().map_err(de::Error::custom)
    }
}
