use super::{LockfileResolution, is_git_hosted_tarball_url};

/// Tarball URL recorded on an npm-registry resolution. The verifier
/// uses it for prefix-matching against named registries; absence
/// alone doesn't disqualify the entry (Registry / Tarball variants
/// without a URL still go through scope routing).
pub(super) fn npm_registry_tarball(resolution: &LockfileResolution) -> Option<Option<&str>> {
    match resolution {
        // Registry-resolved entries carry only `integrity`; the tarball
        // URL is reconstructed at fetch time. They still qualify for
        // verification.
        LockfileResolution::Registry(_) => Some(None),
        LockfileResolution::Tarball(t) => {
            // Git-hosted tarballs (codeload / gitlab / bitbucket) are
            // not subject to the release-age policy and don't have a
            // packument lookup; skip them. The exemption is decided from
            // the URL alone, never from the recorded `gitHosted` flag: the
            // flag is lockfile input, so a tampered entry could otherwise
            // set it on an attacker-hosted URL and buy itself the same
            // exemption.
            if is_git_hosted_tarball_url(&t.tarball) {
                return None;
            }
            if let Ok(parsed) = reqwest::Url::parse(&t.tarball) {
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    return None;
                }
            }
            Some(Some(t.tarball.as_str()))
        }
        // Custom resolutions have no packument lookup — the pnpmfile
        // custom resolver, not the npm registry, is their authority.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => None,
    }
}
