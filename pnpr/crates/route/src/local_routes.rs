use super::{RouteContext, nerf_dart};

impl RouteContext {
    /// Whether a fetch addresses this server's configured public URL.
    #[must_use]
    pub(super) fn is_own_url(&self, url: &str) -> bool {
        self.hosted_origin
            .as_ref()
            .is_some_and(|origin| nerf_dart(url).starts_with(origin))
    }

    pub(super) fn local_package(&self, url: &str) -> Option<String> {
        let url = url.split(['?', '#']).next()?;
        let fetch = nerf_dart(&format!("{url}/"));
        let tail = fetch
            .strip_prefix(self.hosted_origin.as_deref()?)?
            .strip_prefix("npm/")?;
        let decoded = pnpm_network::percent_decode_str(tail);
        let mut segments = decoded.split('/');
        let mut first = segments.next()?;
        if first.starts_with('~') {
            first = segments.next()?;
        }
        let identifier = if first.starts_with('@') {
            format!("{first}/{}", segments.next()?)
        } else if !first.is_empty() && first != "-" {
            first.to_string()
        } else {
            return None;
        };
        Some(
            split_package_identifier(&identifier)
                .map_or_else(|| identifier.clone(), |(name, _)| name.to_string()),
        )
    }
}

/// Split `<name>@<version>` into its parts. A package name holds `@` only as
/// its scope prefix, so the version follows the last `@` past the first byte.
#[must_use]
pub fn split_package_identifier(identifier: &str) -> Option<(&str, &str)> {
    let at = identifier
        .rfind('@')
        .filter(|at| *at > 0)?;
    let (name, version) = (&identifier[..at], &identifier[at + 1..]);
    (!version.is_empty() && !name.ends_with('/')).then_some((name, version))
}

#[cfg(test)]
mod tests;
