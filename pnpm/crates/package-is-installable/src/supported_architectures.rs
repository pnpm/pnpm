//! Which platforms an install prepares for.

pub mod platform;

use platform::{
    Architecture,
    Libc,
    LibcFamily,
    NamedPlatform,
    Os,
    SupportedPlatform,
};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    de::{
        self,
        MapAccess,
        SeqAccess,
        Visitor,
        value::MapAccessDeserializer,
    },
};
use std::fmt;

/// Which platforms an install prepares for, as `supportedArchitectures`
/// names them.
///
/// The axes stand for every combination of the values they name, which is
/// what npm's `os`, `cpu` and `libc` filtering has always meant. A
/// platform list names the platforms themselves, so a workspace that ships
/// on Linux x64 and macOS arm64 prepares for those two rather than for the
/// four an `os` list and a `cpu` list cross into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SupportedArchitectures {
    /// `supportedArchitectures: [linux-x64, darwin-arm64]`.
    Platforms(Vec<SupportedPlatform>),
    /// `supportedArchitectures: {os: [linux], cpu: [x64, arm64]}`.
    Axes(ArchitectureAxes),
}

/// The `os`, `cpu` and `libc` a package's own triple is evaluated
/// against. An axis left unset is the one the install runs on, which is
/// what the `current` sentinel names explicitly.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureAxes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<Vec<String>>,
}

impl Default for SupportedArchitectures {
    fn default() -> Self {
        Self::Axes(ArchitectureAxes::default())
    }
}

impl SupportedArchitectures {
    /// The platforms this setting names, each one once and in the order
    /// it was written, with `current` read as the platform the install
    /// runs on. Resolving it here is what lets a caller record which
    /// platforms a piece of work was done for.
    ///
    /// `current_cpu` is the architecture the install runs on, named as
    /// precisely as the caller can name it: the Node name a package
    /// declares is one name for both POWER endiannesses, so a caller
    /// with the machine's own target-triple spelling should pass that.
    ///
    /// A platform list is validated as it is read, so every entry of one
    /// is here. The axes are read by the optional-dependency check too,
    /// where any name a package may declare is meaningful, so a
    /// combination of them that is not a platform pnpm knows is left out
    /// rather than reported.
    #[must_use]
    pub fn platforms(
        &self,
        current_os: &str,
        current_cpu: &str,
        current_libc: &str,
    ) -> Vec<NamedPlatform> {
        let named = match self {
            Self::Platforms(platforms) => platforms
                .iter()
                .filter_map(|platform| match platform {
                    SupportedPlatform::Current => host(current_os, current_cpu, current_libc),
                    SupportedPlatform::Named(named) => Some(named.clone()),
                })
                .collect(),
            Self::Axes(axes) => axes.cross(current_os, current_cpu, current_libc),
        };
        let mut platforms = Vec::with_capacity(named.len());
        for platform in named {
            if !platforms.contains(&platform) {
                platforms.push(platform);
            }
        }
        platforms
    }
}

impl SupportedArchitectures {
    /// The values this setting names that no platform pnpm knows can
    /// stand for, in the order they were written.
    ///
    /// [`Self::platforms`] leaves these out rather than refusing them,
    /// because the axes are read by the optional-dependency check too,
    /// where a system pnpm has no platform model for is still a name a
    /// package declares. A caller that prepares per platform has to say
    /// what it could not prepare for, since dropping them quietly reads
    /// as having covered them.
    #[must_use]
    pub fn unnamed_platform_values(
        &self,
        current_os: &str,
        current_cpu: &str,
        current_libc: &str,
    ) -> Vec<&str> {
        match self {
            Self::Platforms(platforms) => platforms
                .iter()
                .filter(|platform| {
                    **platform == SupportedPlatform::Current
                        && host(current_os, current_cpu, current_libc).is_none()
                })
                .map(|_| "current")
                .collect(),
            Self::Axes(axes) => {
                let mut unnamed = Vec::new();
                unnamed.extend(unread(axes.os.as_deref(), current_os, Os::parse));
                unnamed.extend(unread(axes.cpu.as_deref(), current_cpu, Architecture::parse));
                unnamed.extend(unread(axes.libc.as_deref(), current_libc, Libc::parse));
                unnamed
            }
        }
    }
}

/// The values of one axis that name nothing a platform can be built
/// from, which is what [`named`] leaves out.
fn unread<'a, Value>(
    values: Option<&'a [String]>,
    current: &str,
    parse: impl Fn(&str) -> Option<Value>,
) -> Vec<&'a str> {
    let Some(values) = values else {
        return Vec::new();
    };
    values
        .iter()
        .map(String::as_str)
        .filter(|value| !reads(value, current, &parse))
        .collect()
}

/// Whether an axis value names something a platform can be built from.
/// `current` names whatever the install runs on.
fn reads<Value>(value: &str, current: &str, parse: impl Fn(&str) -> Option<Value>) -> bool {
    parse(if value == "current" { current } else { value }).is_some()
}

impl SupportedArchitectures {
    /// [`Self::platforms`] for the machine this install runs on.
    ///
    /// Naming that machine takes more care than naming any other: the
    /// architecture has to be read from the spelling that tells the two
    /// POWER endiannesses apart, which the name a package declares does
    /// not. Every caller that prepares per platform goes through here so
    /// that there is one place to get it right.
    #[must_use]
    pub fn host_platforms(&self) -> Vec<NamedPlatform> {
        self.platforms(
            pnpm_detect_libc::host_platform(),
            pnpm_detect_libc::host_target_arch(),
            pnpm_detect_libc::detect().map_or("unknown", |libc| libc.as_str()),
        )
    }
}

/// The platform the install runs on, or `None` on one pnpm cannot name.
fn host(current_os: &str, current_cpu: &str, current_libc: &str) -> Option<NamedPlatform> {
    let os = Os::parse(current_os)?;
    let architecture = Architecture::parse(current_cpu)?;
    let libc = (os == Os::Linux).then(|| linux_libc(current_libc));
    Some(NamedPlatform { os, architecture, libc })
}

impl ArchitectureAxes {
    /// Every platform the axes cross into.
    fn cross(&self, current_os: &str, current_cpu: &str, current_libc: &str) -> Vec<NamedPlatform> {
        let architectures = named(self.cpu.as_deref(), current_cpu, Architecture::parse);
        let mut platforms = Vec::new();
        for os in named(self.os.as_deref(), current_os, Os::parse) {
            let libcs = self.libcs(os, current_libc);
            for architecture in &architectures {
                platforms.extend(
                    libcs
                        .iter()
                        .map(|libc| NamedPlatform {
                            os,
                            architecture: *architecture,
                            libc: libc.clone(),
                        }),
                );
            }
        }
        platforms
    }

    /// The C libraries a platform of this system is built against. Only
    /// Linux has one, and a `libc` axis that names none leaves the
    /// install's own, falling back to glibc where it runs on neither.
    fn libcs(&self, os: Os, current_libc: &str) -> Vec<Option<Libc>> {
        if os != Os::Linux {
            return vec![None];
        }
        let values = self.libc
            .as_deref()
            .filter(|values| !values.is_empty());
        let Some(values) = values else {
            return vec![Some(linux_libc(current_libc))];
        };
        named(Some(values), current_libc, Libc::parse)
            .into_iter()
            .map(Some)
            .collect()
    }
}

/// The C library a Linux platform is built against when its name does
/// not say: the install's own, or glibc where it runs on neither. Both
/// ways of naming the running platform have to reach the same answer, or
/// two spellings of it would be two platforms.
fn linux_libc(current_libc: &str) -> Libc {
    Libc::parse(current_libc).unwrap_or_else(|| Libc::family(LibcFamily::Glibc))
}

/// The values one axis names, each once, with `current` read as the
/// install's own and anything the axis may name but a platform may not
/// left out.
///
/// Naming one value twice asks for the same platform twice, and the
/// axes are crossed, so a repeat left in here would be multiplied by
/// every repeat of every other axis before the crossing is over.
fn named<Value: PartialEq>(
    values: Option<&[String]>,
    current: &str,
    parse: impl Fn(&str) -> Option<Value>,
) -> Vec<Value> {
    let Some(values) = values.filter(|values| !values.is_empty()) else {
        return parse(current).into_iter().collect();
    };
    let mut named = Vec::new();
    for value in values {
        let Some(value) = parse(if value == "current" { current } else { value }) else {
            continue;
        };
        if !named.contains(&value) {
            named.push(value);
        }
    }
    named
}

impl<'de> Deserialize<'de> for SupportedArchitectures {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        deserializer.deserialize_any(SupportedArchitecturesVisitor)
    }
}

/// Written out rather than derived as `#[serde(untagged)]` so that the
/// error from a malformed entry survives: an untagged enum reports only
/// that no variant matched, which would hide which platform pnpm does not
/// know.
struct SupportedArchitecturesVisitor;

impl<'de> Visitor<'de> for SupportedArchitecturesVisitor {
    type Value = SupportedArchitectures;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a list of platforms, or an os, cpu and libc mapping")
    }

    fn visit_seq<Seq: SeqAccess<'de>>(self, mut seq: Seq) -> Result<Self::Value, Seq::Error> {
        let mut platforms = Vec::new();
        while let Some(platform) = seq.next_element()? {
            platforms.push(platform);
        }
        if platforms.is_empty() {
            return Err(de::Error::custom(
                "supportedArchitectures has to name a platform, such as linux-x64",
            ));
        }
        Ok(SupportedArchitectures::Platforms(platforms))
    }

    fn visit_map<Map: MapAccess<'de>>(self, map: Map) -> Result<Self::Value, Map::Error> {
        ArchitectureAxes::deserialize(MapAccessDeserializer::new(map))
            .map(SupportedArchitectures::Axes)
    }
}
