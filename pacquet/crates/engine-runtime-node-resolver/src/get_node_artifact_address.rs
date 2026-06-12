//! Pacquet port of
//! [`getNodeArtifactAddress.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/getNodeArtifactAddress.ts).

use crate::normalize_arch::get_normalized_arch;

/// Three pieces of the full archive URL for one Node.js artifact.
///
/// Splitting them keeps the caller free to compose either the URL
/// (`{dirname}/{basename}{extname}`) or the zip-prefix
/// (`{basename}`) — pnpm's
/// [`BinaryResolution.prefix`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L41-L49)
/// needs only the basename, while the `url` field needs the full
/// concatenation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeArtifactAddress {
    pub basename: String,
    pub extname: String,
    pub dirname: String,
}

/// Options bundle for [`get_node_artifact_address`].
///
/// `libc` only ever takes `Some("musl")` upstream — glibc is the
/// implicit default and is *not* included in the archive name. The
/// type is `Option<&str>` so future libc additions don't churn the
/// API.
#[derive(Debug, Clone, Copy)]
pub struct GetNodeArtifactAddressOptions<'a> {
    pub version: &'a str,
    pub base_url: &'a str,
    pub platform: &'a str,
    pub arch: &'a str,
    pub libc: Option<&'a str>,
}

/// Compose the archive URL pieces for a single Node.js platform variant.
#[must_use]
pub fn get_node_artifact_address(opts: GetNodeArtifactAddressOptions<'_>) -> NodeArtifactAddress {
    let is_windows = opts.platform == "win32";
    let normalized_platform = if is_windows { "win" } else { opts.platform };
    let normalized_arch = get_normalized_arch(opts.platform, opts.arch, Some(opts.version));
    let arch_suffix = if opts.libc == Some("musl") { "-musl" } else { "" };
    NodeArtifactAddress {
        dirname: format!("{base_url}v{version}", base_url = opts.base_url, version = opts.version),
        basename: format!(
            "node-v{version}-{normalized_platform}-{normalized_arch}{arch_suffix}",
            version = opts.version,
        ),
        extname: if is_windows { ".zip".to_string() } else { ".tar.gz".to_string() },
    }
}

#[cfg(test)]
mod tests;
