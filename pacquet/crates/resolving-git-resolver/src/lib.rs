//! Pacquet port of pnpm's
//! [`@pnpm/resolving.git-resolver`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts).
//!
//! Resolves dependencies whose `bareSpecifier` names a git repository:
//! the GitHub / GitLab / Bitbucket short-hands (`github:owner/repo#ref`,
//! `gitlab:…`, `bitbucket:…`, the bare `owner/repo` form), git-scheme
//! URLs (`git+ssh`, `git+https`, `git+file`, plain `ssh`, ...), and the
//! plain `https://host/repo.git[#ref]` shape some hosts (Gitea, ...)
//! serve.
//!
//! Three pieces:
//!
//! - [`create_git_hosted_pkg_id()`] — pure ID builder for git resolutions.
//!   Ports
//!   [`createGitHostedPkgId.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/createGitHostedPkgId.ts).
//! - [`parse_bare_specifier()`] — recognise + normalise the input string,
//!   resolve hosted-vs-private (HTTP HEAD probe + `git ls-remote --exit-code`
//!   reachability check), pick a `fetchSpec`. Ports
//!   [`parseBareSpecifier.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts).
//! - [`GitResolver`] — the [`Resolver`](pacquet_resolving_resolver_base::Resolver)
//!   impl that drives the two above, runs `git ls-remote` to pin a
//!   commit, and emits either a `Tarball{gitHosted: true}` or `Git`
//!   resolution. Ports
//!   [`index.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts).
//!
//! Out of scope:
//!
//! - The `prev_specifier` short-circuit (upstream's `currentPkg && !update`
//!   branch). Pacquet doesn't thread `currentPkg` through the seam yet
//!   — the resolver always re-runs `ls-remote`. Restore the fast path
//!   when `currentPkg` lands on `ResolveOptions`.
//! - Proxy / TLS plumbing on the HTTP HEAD probe — the probe uses the
//!   default [`pacquet_network::ThrottledClient`], same as the rest of
//!   the install path.

mod create_git_hosted_pkg_id;
mod git_resolver;
mod hosted_git;
mod parse_bare_specifier;
mod resolve_ref;
mod runners;

pub use create_git_hosted_pkg_id::create_git_hosted_pkg_id;
pub use git_resolver::GitResolver;
pub use hosted_git::{HostedGit, HostedGitType, HostedOpts};
pub use parse_bare_specifier::{
    GitProbe, HostedPackageSpec, PartialSpec, ProbeFuture, parse_bare_specifier,
};
pub use resolve_ref::{GitCommandRunner, GitResolveRefError, GitRunError, resolve_ref};
pub use runners::{RealGitProbe, RealGitRunner};
