use super::{
    AsciiSet, Diagnostic, Digest, Display, Error, NON_ALPHANUMERIC, Sha256, percent_decode_str,
    redact_and_sanitize, redact_url_for_display, utf8_percent_encode,
};
use std::fmt::Write as _;

/// Failure parsing a registry URL into a filesystem-safe slug.
/// Real-world registries always carry a host; this only triggers on
/// malformed config. `url` is redacted for display — a registry can
/// carry `user:pass@` credentials that must not reach a CI log.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EncodeRegistryError {
    #[display("Failed to parse registry URL {url:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INVALID_REGISTRY_URL))]
    ParseUrl {
        #[error(not(source))]
        url: String,
        error: String,
    },
    #[display("Registry URL {url:?} has no host")]
    #[diagnostic(code(ERR_PNPM_MISSING_REGISTRY_HOST))]
    MissingHost {
        #[error(not(source))]
        url: String,
    },
}

/// Everything a registry key does not carry verbatim. Escaping all of
/// it means a `%` or `+` in the key is always one [`get_registry_name`]
/// wrote, and the key can never contain a path separator, a character
/// Windows rejects in a filename, or a glob metacharacter — the cache
/// commands interpolate the key straight into a glob pattern, and
/// `pnpm cache delete` erases whatever that pattern matches.
pub(super) const ESCAPED_IN_REGISTRY_KEY: &AsciiSet =
    &NON_ALPHANUMERIC.remove(b'.').remove(b'-').remove(b'_');

/// Separates the scheme from the host. Every key begins with one, and a
/// scheme cannot contain a `%`, so the first occurrence always ends the
/// scheme.
pub(super) const SCHEME_SEPARATOR: &str = "%3A+";

/// Separates the host from the path. A percent-escape, so that nothing an
/// escaped component can spell is mistaken for it.
pub(super) const PATH_SEPARATOR: &str = "%2F";

/// Separates the key from the hash that disambiguates a mixed-case path.
pub(super) const HASH_SEPARATOR: &str = "%5F";

/// The 255-byte limit on one filename that ext4, APFS and NTFS share. A
/// registry path long enough to exceed it would make every mirror read
/// miss and every mirror write fail, so such a key collapses to its own
/// hash: still one directory per registry, just no longer readable.
pub(super) const MAX_KEY_LENGTH: usize = 255;

/// Directory name under the metadata cache root that holds a registry's
/// mirrored packuments, shaped `<host>[+<port>][%2F<path>][%5F<hash>]`.
///
/// `+` joins the port to the host and the path segments to one another,
/// and every other occurrence of it is escaped, as is every character
/// that could spell one of the two separators — so no two registry URLs
/// share a directory. They must not: a shared directory lets the resolver
/// answer
/// from another registry's versions, integrity hashes and tarball URLs,
/// which surfaces as `ERR_PNPM_TARBALL_URL_MISMATCH` when the lockfile is
/// verified.
///
/// The port is dropped when it is the scheme's default and leading and
/// trailing slashes are trimmed, so the same registry spelled
/// `https://r/`, `https://r:443` and `https://r:443/` keeps one cache
/// rather than three. A path that is not all lowercase gets a sha256
/// suffix, the guard [`encode_pkg_name`] applies to package names,
/// because HFS+ and NTFS would otherwise merge `…/Team` into `…/team`. A
/// trailing `.` is escaped because Win32 strips one, which would alias
/// `…/foo.` onto `…/foo`. A key that would not fit a 255-byte filename is
/// replaced by its own hash.
pub fn get_registry_name(registry: &str) -> Result<String, EncodeRegistryError> {
    let parsed = reqwest::Url::parse(registry).map_err(|error| EncodeRegistryError::ParseUrl {
        url: redact_and_sanitize(registry),
        error: error.to_string(),
    })?;
    let host = parsed.host_str().ok_or_else(|| EncodeRegistryError::MissingHost {
        url: redact_url_for_display(registry),
    })?;
    let mut key = escape_registry_key_component(parsed.scheme());
    key.push_str(SCHEME_SEPARATOR);
    key.push_str(&escape_registry_key_component(host));
    if let Some(port) = parsed.port() {
        write!(key, "+{port}").expect("writing to a String never fails");
    }
    append_path_key(&mut key, &path_segments(parsed.path()));
    if key.ends_with('.') {
        key.pop();
        key.push_str("%2E");
    }
    if key.len() > MAX_KEY_LENGTH {
        let digest = Sha256::digest(key.as_bytes());
        key = format!("{digest:x}");
    }
    Ok(key)
}

/// Append the registry path's own key. A path that is not all lowercase
/// gets a sha256 suffix, because HFS+ and NTFS would otherwise merge
/// `…/Team` into `…/team`.
pub(super) fn append_path_key(key: &mut String, segments: &[&str]) {
    if segments.is_empty() {
        return;
    }
    key.push_str(PATH_SEPARATOR);
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            key.push('+');
        }
        key.push_str(&escape_registry_key_component(segment));
    }
    let path = segments.join("/");
    if path.to_lowercase() != path {
        let digest = Sha256::digest(path.as_bytes());
        write!(key, "{HASH_SEPARATOR}{digest:x}").expect("writing to a String never fails");
    }
}

/// The registry a key made by [`get_registry_name`] came from. The sha256
/// that separates two paths differing only in case is not part of the
/// registry, so it is dropped.
///
/// The result is the registry in the trailing-slashed form the resolver
/// normalizes to, which is what makes it the exact inverse: `…%2F` names
/// `https://r//`, one slash more than `…` alone.
///
/// `pnpm cache view` labels its output with this. A key carrying no
/// scheme separator decodes to the `host[:port]` it names, so a cache root
/// holding both shapes still labels every entry, and anything that does
/// not decode is returned unchanged rather than erroring.
#[must_use]
pub fn decode_registry_name(registry_key: &str) -> String {
    let (scheme, authority) = match registry_key.split_once(SCHEME_SEPARATOR) {
        None => (None, registry_key),
        Some((scheme, authority)) => (Some(scheme), authority),
    };
    let (host, path) = match authority.split_once(PATH_SEPARATOR) {
        None => (authority, None),
        Some((host, rest)) => {
            (host, Some(rest.split_once(HASH_SEPARATOR).map_or(rest, |(path, _hash)| path)))
        }
    };
    let Some(host) = decode_registry_key_component(host, ":") else {
        return registry_key.to_string();
    };
    let Some(scheme) = scheme else {
        return host;
    };
    let Some(scheme) = decode_registry_key_component(scheme, ":") else {
        return registry_key.to_string();
    };
    match path {
        None => format!("{scheme}://{host}/"),
        Some(path) => match decode_registry_key_component(path, "/") {
            Some(path) => format!("{scheme}://{host}/{path}/"),
            None => registry_key.to_string(),
        },
    }
}

/// `None` when the component is not a well-formed percent-encoding of
/// UTF-8, which leaves the caller the same choice `decodeRegistry` makes
/// in pnpm v11: hand back the key untouched rather than a lossy or
/// half-decoded rendering of it. `percent_decode_str` alone would not do:
/// it passes a malformed escape through as literal text, where
/// `decodeURIComponent` throws.
pub(super) fn decode_registry_key_component(component: &str, delimiter: &str) -> Option<String> {
    let replaced = component.replace('+', delimiter);
    if !percent_escapes_are_well_formed(&replaced) {
        return None;
    }
    percent_decode_str(&replaced).decode_utf8().ok().map(std::borrow::Cow::into_owned)
}

/// Whether every `%` in `text` introduces two hexadecimal digits.
pub(super) fn percent_escapes_are_well_formed(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let Some(offset) = bytes[index..].iter().position(|byte| *byte == b'%') else {
            return true;
        };
        let start = index + offset;
        let Some(digits) = bytes.get(start + 1..start + 3) else {
            return false;
        };
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return false;
        }
        index = start + 3;
    }
    true
}

/// The path a registry addresses, as the segments between its slashes.
///
/// The only spelling difference that does not reach the registry is a
/// missing trailing slash, because the resolver appends one to a registry
/// configured without it — so `…/a` and `…/a/` share a cache. Every other
/// slash is significant: `https://r/`, `https://r//` and `https://r///`
/// request `/lodash`, `//lodash` and `///lodash` respectively, so they are
/// three registries and get three directories, carrying one, two and three
/// segments.
pub(super) fn path_segments(path: &str) -> Vec<&str> {
    // Every path opens with the `/` that [`PATH_SEPARATOR`] stands for, so its
    // empty head is dropped; a trailing `/` is the one slash the resolver
    // normalizes away, so its empty tail goes with it.
    let mut segments: Vec<&str> = path.split('/').skip(1).collect();
    if path.ends_with('/') {
        segments.pop();
    }
    segments
}

pub(super) fn escape_registry_key_component(component: &str) -> String {
    utf8_percent_encode(component, ESCAPED_IN_REGISTRY_KEY).to_string()
}

/// Filesystem-safe form of a package name. A mixed-case name gets a
/// sha256 hex suffix so case-insensitive filesystems (HFS+, NTFS by
/// default) can't collide it with a lowercase sibling.
#[must_use]
pub fn encode_pkg_name(pkg_name: &str) -> String {
    let lowered = pkg_name.to_lowercase();
    if pkg_name == lowered {
        return pkg_name.to_string();
    }
    let digest = Sha256::digest(pkg_name.as_bytes());
    format!("{pkg_name}_{digest:x}")
}
