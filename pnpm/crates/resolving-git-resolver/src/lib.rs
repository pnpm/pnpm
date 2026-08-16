//! Resolves dependencies whose `bareSpecifier` names a git repository:
//! the GitHub / GitLab / Bitbucket short-hands (`github:owner/repo#ref`,
//! `gitlab:…`, `bitbucket:…`, the bare `owner/repo` form), git-scheme
//! URLs (`git+ssh`, `git+https`, `git+file`, plain `ssh`, ...), and the
//! plain `https://host/repo.git[#ref]` shape some hosts (Gitea, ...)
//! serve.
//!
//! Specs of known hosts are identities, not transport choices —
//! `parse_bare_specifier`'s module doc states the rule.
//!
//! Three pieces:
//!
//! - [`create_git_hosted_pkg_id()`] — pure ID builder for git resolutions.
//! - [`parse_bare_specifier()`] — recognise + normalise the input
//!   string, pick a `fetchSpec`. Pure — no network.
//! - [`GitResolver`] — the [`Resolver`](pnpm_resolving_resolver_base::Resolver)
//!   impl that runs `git ls-remote` to pin a commit and emits either a
//!   `Tarball{gitHosted: true}` or `Git` resolution, decided by the
//!   [`GitProbe`] archive check. Given a [`GitFetchContext`], it also
//!   reads the package's name from its `package.json` — out of the
//!   host archive, or out of a checkout for a repo that serves no
//!   archive — plus the archive's integrity. See that type for why
//!   resolution is where that has to happen.
//!
//! Out of scope:
//!
//! - The `prev_specifier` short-circuit (the `currentPkg && !update`
//!   fast path). Pacquet doesn't thread `currentPkg` through the seam
//!   yet — the resolver always re-runs `ls-remote`. Restore the fast
//!   path when `currentPkg` lands on `ResolveOptions`.
//! - Proxy / TLS plumbing on the HTTP HEAD probe — the probe uses the
//!   default [`pnpm_network::ThrottledClient`], same as the rest of
//!   the install path.

mod create_git_hosted_pkg_id;
mod git_resolver;
mod hosted_git;
mod parse_bare_specifier;
mod resolve_ref;
mod runners;

pub use create_git_hosted_pkg_id::create_git_hosted_pkg_id;
pub use git_resolver::{GitFetchContext, GitProbe, GitResolver, ProbeFuture};
pub use hosted_git::{HostedGit, HostedGitType, HostedOpts};
pub use parse_bare_specifier::{HostedPackageSpec, PartialSpec, parse_bare_specifier};
pub use resolve_ref::{
    GitCommandRunner, GitResolveRefError, GitRunError, get_repo_refs, resolve_ref,
};
pub use runners::{RealGitProbe, RealGitRunner};
