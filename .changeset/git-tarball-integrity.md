---
"@pnpm/git-resolver": patch
"@pnpm/license-scanner": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.types": patch
"@pnpm/lockfile.utils": patch
"@pnpm/package-requester": patch
"@pnpm/pick-fetcher": patch
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/plugin-commands-store": patch
"@pnpm/resolve-dependencies": patch
"@pnpm/resolver-base": patch
"@pnpm/tarball-fetcher": patch
"pnpm": patch
---

Pin the integrity of git-hosted tarballs (codeload.github.com, gitlab.com, bitbucket.org) in the lockfile so that subsequent installs detect a tampered or substituted tarball and refuse to install it. Previously the lockfile only stored the tarball URL for git dependencies, so a compromised git host or a man-in-the-middle could serve arbitrary code on later installs without lockfile changes.

A new `gitHosted: true` field is recorded on git-hosted tarball resolutions in the lockfile, letting every reader/writer route them by a single typed check instead of pattern-matching the tarball URL in each call site. Lockfiles written by older pnpm versions are enriched on load (URL fallback) so the field can be relied on uniformly across the codebase.
