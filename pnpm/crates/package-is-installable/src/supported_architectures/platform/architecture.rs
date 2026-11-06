//! The machine architectures a platform can name, each spelled the three
//! ways packages, platforms, and Python wheels spell it.

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
    // Little-endian comes first so that a bare `ppc64`, which is the
    // name Node reports for either and the one a package declares,
    // reads as the endianness every POWER machine pnpm runs on has.
    ArchitectureNames {
        architecture: Architecture::Ppc64Le,
        platform: "ppc64le",
        cpu: "ppc64",
        wheel: "ppc64le",
        aliases: &["powerpc64le"],
    },
    ArchitectureNames {
        architecture: Architecture::Ppc64Be,
        platform: "ppc64be",
        cpu: "ppc64",
        wheel: "ppc64",
        aliases: &["powerpc64"],
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
    Ppc64Le,
    /// Big-endian POWER, which a platform has to name `ppc64be` to ask
    /// for. Node reports `ppc64` for either endianness, so that is the
    /// one name a package declares and it reads as the little-endian
    /// platform, which is the one wheels are published for.
    Ppc64Be,
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

    pub(super) fn platform_name(self) -> &'static str {
        self.names().platform
    }

    fn names(self) -> &'static ArchitectureNames {
        ARCHITECTURES
            .iter()
            .find(|names| names.architecture == self)
            .expect("every architecture is in ARCHITECTURES")
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
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
