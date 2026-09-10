//! The subset of
//! [`hosted-git-info`](https://github.com/npm/hosted-git-info/tree/v4.1.0)
//! that pacquet's git resolver uses. v4.1.0 is the major pinned in the
//! root `package.json` (catalog entry `hosted-git-info: ^4.1.0`) and is
//! what `node_modules/hosted-git-info/` ships.
//!
//! Deliberate deviations from upstream hosted-git-info:
//!
//! - The GitLab tarball template emits `/-/archive/<ref>/<project>-<ref>.tar.gz`
//!   directly. Upstream hosted-git-info still emits the
//!   `/api/v4/projects/<user>%2F<project>/repository/archive.tar.gz`
//!   form; pacquet uses the override, not the raw template.
//! - The `gist` host is not implemented. The test suite never
//!   exercises it and the install path has no gist-shaped store key.
//! - `browse` / `bugs` / `file` / `git` templates are not implemented.

mod url_parse;
use url_parse::{
    correct_protocol, extract_auth, host_segments, is_github_shorthand, parse_git_url,
    percent_decode, shortcut_segments,
};

use pnpm_network::encode_uri_component;
use std::fmt;

/// Three host families pacquet recognises. Mirrors upstream's
/// `gitHosts` keys at
/// <https://github.com/npm/hosted-git-info/blob/v4.1.0/git-host-info.js>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedGitType {
    Github,
    Gitlab,
    Bitbucket,
}

impl HostedGitType {
    fn domain(self) -> &'static str {
        match self {
            HostedGitType::Github => "github.com",
            HostedGitType::Gitlab => "gitlab.com",
            HostedGitType::Bitbucket => "bitbucket.org",
        }
    }

    fn shortcut_prefix(self) -> &'static str {
        match self {
            HostedGitType::Github => "github",
            HostedGitType::Gitlab => "gitlab",
            HostedGitType::Bitbucket => "bitbucket",
        }
    }

    fn from_shortcut(scheme: &str) -> Option<HostedGitType> {
        match scheme {
            "github" => Some(HostedGitType::Github),
            "gitlab" => Some(HostedGitType::Gitlab),
            "bitbucket" => Some(HostedGitType::Bitbucket),
            _ => None,
        }
    }

    fn from_domain(host: &str) -> Option<HostedGitType> {
        // Strip leading `www.` to match upstream's
        // `parsed.hostname.startsWith('www.') ? parsed.hostname.slice(4) : parsed.hostname`.
        let host = host.strip_prefix("www.").unwrap_or(host);
        match host {
            "github.com" => Some(HostedGitType::Github),
            "gitlab.com" => Some(HostedGitType::Gitlab),
            "bitbucket.org" => Some(HostedGitType::Bitbucket),
            _ => None,
        }
    }

    fn supports_protocol(self, proto: &str) -> bool {
        match self {
            // gitHosts.github.protocols
            HostedGitType::Github => {
                matches!(proto, "git" | "http" | "git+ssh" | "git+https" | "ssh" | "https")
            }
            // gitHosts.gitlab.protocols and gitHosts.bitbucket.protocols
            HostedGitType::Gitlab | HostedGitType::Bitbucket => {
                matches!(proto, "git+ssh" | "git+https" | "ssh" | "https")
            }
        }
    }
}

/// Parsed git host info. Mirrors upstream's `GitHost` instance fields
/// (sans the unused `default` / `opts` slots).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedGit {
    pub host_type: HostedGitType,
    pub user: String,
    pub auth: Option<String>,
    pub project: String,
    pub committish: Option<String>,
    /// The original protocol the URL came in with. Drives the
    /// "default representation" upstream picks for `toString` /
    /// `shortcut` round-trips.
    default_representation: Representation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Representation {
    Shortcut,
    Sshurl,
    Https,
    Git,
    Http,
}

/// Per-call options for the `_fill`-style URL templates.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostedOpts {
    /// Drop the trailing `#<committish>` segment.
    pub no_committish: bool,
    /// Strip the leading `git+` from `https` / `ssh` outputs.
    pub no_git_plus: bool,
}

impl HostedGit {
    /// Convenience: build options that omit the committish.
    #[must_use]
    pub fn no_committish() -> HostedOpts {
        HostedOpts { no_committish: true, no_git_plus: false }
    }

    /// Convenience: drop both `#commit` and the `git+` prefix.
    #[must_use]
    pub fn no_committish_no_git_plus() -> HostedOpts {
        HostedOpts { no_committish: true, no_git_plus: true }
    }
}

impl HostedGit {
    /// Recognise a git URL the way upstream's
    /// [`fromUrl`](https://github.com/npm/hosted-git-info/blob/v4.1.0/index.js#L29-L41)
    /// does.
    ///
    /// Returns `None` when the input names a host pacquet doesn't
    /// recognise (Gitea, self-hosted GitLab, a generic
    /// `git+file://…`, ...), the project is missing, or the URL parses
    /// to an unsupported shape (e.g. a bitbucket `/get/…` archive
    /// URL — upstream's `extract` returns undefined for those and
    /// pacquet mirrors it).
    pub fn from_url(giturl: &str) -> Option<HostedGit> {
        if giturl.is_empty() {
            return None;
        }
        // GitHub shorthand: prepend `github:` and run through the
        // shortcut path. Mirrors upstream's
        // `isGitHubShorthand(giturl) ? 'github:' + giturl : correctProtocol(giturl)`.
        let normalised = if is_github_shorthand(giturl) {
            format!("github:{giturl}")
        } else {
            correct_protocol(giturl)
        };

        let parsed = parse_git_url(&normalised)?;
        // Look up host: shortcut first (so `github://...` wins over the
        // host's full URL parsing), then by domain.
        let shortcut_type = HostedGitType::from_shortcut(&parsed.scheme);
        let domain_type = parsed.host.as_deref().and_then(HostedGitType::from_domain);
        let host_type = shortcut_type.or(domain_type)?;

        let segments = if shortcut_type.is_some() {
            shortcut_segments(&parsed)
        } else {
            host_segments(host_type, &parsed)?
        };

        if segments.project.is_empty() {
            return None;
        }

        Some(HostedGit {
            host_type,
            user: segments.user,
            auth: extract_auth(&parsed),
            project: segments.project,
            committish: segments.committish,
            default_representation: segments.representation,
        })
    }

    /// Shorthand `<type>:<user>/<project>[#committish]`. Mirrors
    /// upstream's `shortcuttemplate`.
    #[must_use]
    pub fn shortcut(&self, opts: HostedOpts) -> String {
        let mut out =
            format!("{}:{}/{}", self.host_type.shortcut_prefix(), self.user, self.project);
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        out
    }

    /// `git+https://[auth@]<domain>/<user>/<project>.git[#committish]`,
    /// optionally stripped of `git+`. Mirrors upstream's
    /// `httpstemplate` (gitlab and github share the same shape).
    #[must_use]
    pub fn https(&self, opts: HostedOpts) -> Option<String> {
        let auth = self.auth.as_deref().map(|a| format!("{a}@")).unwrap_or_default();
        let mut out = format!(
            "git+https://{auth}{domain}/{user}/{project}.git",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        );
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        if opts.no_git_plus
            && let Some(stripped) = out.strip_prefix("git+")
        {
            out = stripped.to_string();
        }
        Some(out)
    }

    /// Package documentation URL matching normalize-package-data.
    #[must_use]
    pub fn package_docs_url(giturl: &str) -> Option<String> {
        let mut hosted = Self::from_url(giturl)?;
        if hosted.host_type == HostedGitType::Github
            && let Some((_, tree_path)) = giturl.split_once("/tree/")
            && let Some(committish) = tree_path.split(['/', '#', '?']).next()
        {
            hosted.committish = Some(percent_decode(committish));
        }
        Some(hosted.docs())
    }

    fn docs(&self) -> String {
        if let Some(committish) = &self.committish {
            let separator = match self.host_type {
                HostedGitType::Github | HostedGitType::Gitlab => "tree",
                HostedGitType::Bitbucket => "src",
            };
            return format!(
                "https://{domain}/{user}/{project}/{separator}/{committish}#readme",
                domain = self.host_type.domain(),
                user = self.user,
                project = self.project,
                committish = encode_uri_component(committish),
            );
        }
        format!(
            "https://{domain}/{user}/{project}#readme",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        )
    }

    /// `git@<domain>:<user>/<project>.git[#committish]`. Mirrors
    /// upstream's `sshtemplate`.
    #[must_use]
    pub fn ssh(&self, opts: HostedOpts) -> Option<String> {
        let mut out = format!(
            "git@{domain}:{user}/{project}.git",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        );
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        Some(out)
    }

    /// `git+ssh://git@<domain>/<user>/<project>.git[#committish]`.
    /// Mirrors upstream's `sshurltemplate`.
    #[must_use]
    pub fn sshurl(&self, opts: HostedOpts) -> Option<String> {
        let mut out = format!(
            "git+ssh://git@{domain}/{user}/{project}.git",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        );
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        if opts.no_git_plus
            && let Some(stripped) = out.strip_prefix("git+")
        {
            out = stripped.to_string();
        }
        Some(out)
    }

    /// Host-specific tarball URL. Mirrors upstream's `tarballtemplate`
    /// per host, with one deviation: GitLab uses the
    /// `/-/archive/<ref>/<project>-<ref>.tar.gz` shape instead of the
    /// upstream template.
    ///
    /// Returns `None` when no committish is set — every supported host
    /// uses an explicit ref or the literal `HEAD` / `master` placeholder
    /// from upstream's template. Pacquet only ever invokes
    /// `tarball()` after [`crate::resolve_ref::resolve_ref`] has pinned
    /// the commit, so the `None` here is precautionary.
    #[must_use]
    pub fn tarball(&self, opts: HostedOpts) -> Option<String> {
        // Upstream `tarball()` overrides `noCommittish: false`; even
        // when the caller asks to drop the committish elsewhere, the
        // tarball needs a ref. Pacquet mirrors that policy: ignore
        // `opts.no_committish` here.
        let _ = opts;
        let committish = self.committish.as_deref()?;
        let encoded_committish = encode_uri_component(committish);
        Some(match self.host_type {
            HostedGitType::Github => format!(
                "https://codeload.github.com/{user}/{project}/tar.gz/{ref}",
                user = self.user,
                project = self.project,
                r#ref = encoded_committish,
            ),
            HostedGitType::Bitbucket => format!(
                "https://bitbucket.org/{user}/{project}/get/{ref}.tar.gz",
                user = self.user,
                project = self.project,
                r#ref = encoded_committish,
            ),
            HostedGitType::Gitlab => format!(
                "https://gitlab.com/{user}/{project}/-/archive/{ref}/{project}-{ref}.tar.gz",
                user = self.user,
                project = self.project,
                r#ref = encoded_committish,
            ),
        })
    }
}

impl fmt::Display for HostedGit {
    /// Mirrors upstream's `toString`: emit the URL form matching the
    /// default representation; fall back to `sshurl` when the default
    /// isn't a render-able URL (e.g. `shortcut`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opts = HostedOpts::default();
        let rendered = match self.default_representation {
            Representation::Sshurl => self.sshurl(opts),
            Representation::Https | Representation::Http => self.https(opts),
            Representation::Git => self.https(opts),
            Representation::Shortcut => Some(self.shortcut(opts)),
        };
        let rendered = rendered.unwrap_or_else(|| self.shortcut(opts));
        f.write_str(&rendered)
    }
}

#[cfg(test)]
mod tests;
