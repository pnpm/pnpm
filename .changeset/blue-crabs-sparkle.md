---
"@pnpm/git-fetcher": patch
"@pnpm/tarball-fetcher": patch
"pnpm": patch
---

Fixes a regression introduced in pnpm v6.23.3 via [#4044](https://github.com/pnpm/pnpm/pull/4044).

The temporary directory to which the Git-hosted package is downloaded should not be removed prematurely [#4064](https://github.com/pnpm/pnpm/issues/4064).
