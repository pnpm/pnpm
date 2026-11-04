//! One platform `supportedArchitectures` names.

pub use architecture::Architecture;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_platform_variants::{Floor, Platform};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

mod aliases;
mod architecture;

/// One platform an install prepares for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportedPlatform {
    /// `current`: the platform the install runs on.
    Current,
    Named(NamedPlatform),
}

/// A platform named outright: a platform tag, as package variants name
/// their platforms, or a spelling pnpm read before it read tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedPlatform {
    /// The platform in the tag vocabulary pnpr shares: `os` and `cpu` as
    /// packages declare them, the C library of a Linux platform (glibc when
    /// it names none), and the floors and ABI it states.
    pub platform: Platform,
    /// The machine's architecture, when pnpm knows it: it tells apart the
    /// two POWER endiannesses that `cpu` names alike, and gives a Python
    /// wheel's spelling.
    pub architecture: Option<Architecture>,
    /// A `musllinux_<major>_<minor>` wheel baseline as written, which the
    /// tag vocabulary has no musl floor for.
    pub musllinux: Option<String>,
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
            "Name a platform as a platform tag, <os>-<cpu> with the C library and floors it needs, such as linux-x64, linux-x64-musl, linux-x64-manylinux_2_28, darwin-arm64-macos_14_0, linux-x64-napi8 or freebsd-x64."
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

    pub(crate) fn parse(name: &str) -> Option<Self> {
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

    pub(crate) fn parse(token: &str) -> Option<Self> {
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

/// The C library a Python wheel baseline is built against, for a value
/// that reads as one: a family and the two numbers of a libc release.
///
/// Which releases were ever published is the interpreter's to know, and
/// it reports a number outside them. That a baseline carries two numbers
/// at all is what tells one from a typo, and catching that here is what
/// keeps a typo a configuration error rather than something the
/// interpreter is asked to make sense of.
fn baseline_family(token: &str) -> Option<LibcFamily> {
    let (family, release) = token.split_once('_')?;
    let (major, minor) = release.split_once('_')?;
    if !is_number(major) || !is_number(minor) {
        return None;
    }
    match family {
        "manylinux" => Some(LibcFamily::Glibc),
        "musllinux" => Some(LibcFamily::Musl),
        _ => None,
    }
}

fn is_number(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

impl NamedPlatform {
    /// The platform of `os` on `architecture`, built against `libc` on
    /// Linux.
    #[must_use]
    pub fn from_machine(os: Os, architecture: Architecture, libc: Option<Libc>) -> Self {
        let libc = libc.filter(|_| os == Os::Linux);
        let baseline = libc.as_ref().and_then(|libc| libc.baseline.clone());
        let family = libc
            .as_ref()
            .map(|libc| match libc.family {
                LibcFamily::Glibc => pnpm_platform_variants::Libc::Glibc,
                LibcFamily::Musl => pnpm_platform_variants::Libc::Musl,
            });
        let musl = family == Some(pnpm_platform_variants::Libc::Musl);
        let platform = Platform {
            os: Some(os.name().to_string()),
            cpu: Some(architecture.cpu().to_string()),
            libc: family,
            libc_version: baseline
                .as_deref()
                .filter(|_| !musl)
                .and_then(manylinux_floor),
            ..Platform::default()
        };
        Self { platform, architecture: Some(architecture), musllinux: baseline.filter(|_| musl) }
    }

    /// The name packages declare under `os`.
    #[must_use]
    pub fn os(&self) -> &str {
        self.platform.os.as_deref().unwrap_or_default()
    }

    /// The name packages declare under `cpu`.
    #[must_use]
    pub fn cpu(&self) -> &str {
        self.platform.cpu.as_deref().unwrap_or_default()
    }

    /// The name packages declare under `libc`, on Linux.
    #[must_use]
    pub fn libc(&self) -> Option<&'static str> {
        self.platform.libc.map(|libc| match libc {
            pnpm_platform_variants::Libc::Glibc => LibcFamily::Glibc.name(),
            pnpm_platform_variants::Libc::Musl => LibcFamily::Musl.name(),
        })
    }

    /// The system and architecture, when the platform is a machine pnpm
    /// knows the Python wheels of.
    #[must_use]
    pub fn machine(&self) -> Option<(Os, Architecture)> {
        Some((Os::parse(self.os())?, self.architecture?))
    }

    /// The `manylinux` or `musllinux` baseline a Python wheel for this
    /// platform is built against, when the platform states one.
    #[must_use]
    pub fn wheel_baseline(&self) -> Option<String> {
        if let Some(musllinux) = &self.musllinux {
            return Some(musllinux.clone());
        }
        let Floor { major, minor } = self.platform.libc_version?;
        Some(format!("manylinux_{major}_{minor}"))
    }
}

/// The glibc floor a `manylinux_<major>_<minor>` baseline states.
fn manylinux_floor(baseline: &str) -> Option<Floor> {
    let (major, minor) = baseline.strip_prefix("manylinux_")?.split_once('_')?;
    Some(Floor { major: major.parse().ok()?, minor: minor.parse().ok()? })
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
    /// The platform's tag, with the architecture and `musllinux` baseline
    /// spelled as they were read, so a platform keeps the name it had
    /// before pnpm read tags.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = self.platform.tag().map_err(|_| fmt::Error)?;
        let mut parts: Vec<&str> = tag.split('-').collect();
        if let (Some(architecture), Some(cpu)) = (self.architecture, parts.get_mut(1)) {
            *cpu = architecture.platform_name();
        }
        if let Some(musllinux) = &self.musllinux {
            for part in &mut parts {
                if *part == "musl" {
                    *part = musllinux;
                }
            }
        }
        formatter.write_str(&parts.join("-"))
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
    let spelled = aliases::canonical(entry)?;
    let platform = Platform::parse_tag(&spelled.tag).map_err(|_| unknown(entry))?;
    if platform.wasm32 {
        return Err(unknown(entry));
    }
    Ok(NamedPlatform { platform, architecture: spelled.architecture, musllinux: spelled.musllinux })
}

pub(super) fn unknown(entry: &str) -> UnknownPlatformError {
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
