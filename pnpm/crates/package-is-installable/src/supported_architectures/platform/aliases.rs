//! The spellings a `supportedArchitectures` entry had before pnpm read
//! platform tags, rewritten into the tag they name: an architecture's other
//! names (`amd64`, `x86_64`), `glibc` or `gnu` for the C library a Linux tag
//! leaves unsaid, a `musllinux` wheel baseline, and the Rust target triple
//! of the same machine.

use super::{Architecture, Libc, LibcFamily, Os, UnknownPlatformError, unknown};
use crate::node_platform_names::{is_node_cpu, is_node_os};

/// An entry as a platform tag, with what the tag cannot carry.
pub(super) struct Spelled {
    pub(super) tag: String,
    pub(super) architecture: Option<Architecture>,
    pub(super) musllinux: Option<String>,
}

/// `entry` as the platform tag it names.
pub(super) fn canonical(entry: &str) -> Result<Spelled, UnknownPlatformError> {
    let (head, rest) = entry.split_once('-').ok_or_else(|| unknown(entry))?;
    if is_node_os(head) {
        return platform_name(entry, head, rest);
    }
    let linux_tag = target_triple_tag(entry, head, rest)?;
    let (os, rest) = linux_tag.split_once('-').ok_or_else(|| unknown(entry))?;
    platform_name(entry, os, rest)
}

/// `<os>-<cpu>[-<libc>][-<floor>…]`, the cpu by any of its names.
fn platform_name(entry: &str, os: &str, rest: &str) -> Result<Spelled, UnknownPlatformError> {
    let mut parts = rest.split('-');
    let cpu_name = parts.next().unwrap_or_default();
    let architecture = Architecture::parse(cpu_name);
    let cpu = match architecture {
        Some(architecture) => architecture.cpu(),
        None if is_node_cpu(cpu_name) => cpu_name,
        None => return Err(unknown(entry)),
    };
    let mut tag = format!("{os}-{cpu}");
    let mut musllinux = None;
    for part in parts {
        match libc_part(entry, os, part)? {
            LibcPart::Unsaid => {}
            LibcPart::Musllinux => {
                tag.push_str("-musl");
                musllinux = Some(part.to_string());
            }
            LibcPart::Tag => {
                tag.push('-');
                tag.push_str(part);
            }
        }
    }
    Ok(Spelled { tag, architecture, musllinux })
}

/// How one part after `<os>-<cpu>` is written in the tag.
enum LibcPart {
    /// glibc, which a Linux tag leaves unsaid.
    Unsaid,
    /// A `musllinux` baseline, written as `musl`.
    Musllinux,
    /// The part as it is: `musl`, a floor, or something the tag parser
    /// refuses.
    Tag,
}

fn libc_part(entry: &str, os: &str, part: &str) -> Result<LibcPart, UnknownPlatformError> {
    let libc = match part {
        "musl" | "glibc" | "gnu" => Libc::parse(part),
        _ if part.starts_with("manylinux") || part.starts_with("musllinux") => {
            Some(Libc::parse(part).ok_or_else(|| unknown(entry))?)
        }
        _ => return Ok(LibcPart::Tag),
    };
    if os != Os::Linux.name() {
        return Err(UnknownPlatformError::LibcOnNonLinux { platform: entry.to_string() });
    }
    Ok(match libc {
        Some(Libc {
            family: LibcFamily::Glibc,
            baseline: None,
        }) => LibcPart::Unsaid,
        Some(Libc {
            family: LibcFamily::Musl,
            baseline: Some(_),
        }) => LibcPart::Musllinux,
        _ => LibcPart::Tag,
    })
}

/// The Rust target triple of a machine, which is how uv and the rest of
/// the Python tooling name the same platform, as `<os>-<cpu>[-<libc>]`.
fn target_triple_tag(
    entry: &str,
    architecture: &str,
    system: &str,
) -> Result<String, UnknownPlatformError> {
    Architecture::parse(architecture).ok_or_else(|| unknown(entry))?;
    Ok(match system {
        "apple-darwin" => format!("darwin-{architecture}"),
        "pc-windows-msvc" => format!("win32-{architecture}"),
        _ => {
            let libc = system.strip_prefix("unknown-linux-").unwrap_or(system);
            Libc::parse(libc).ok_or_else(|| unknown(entry))?;
            format!("linux-{architecture}-{libc}")
        }
    })
}
