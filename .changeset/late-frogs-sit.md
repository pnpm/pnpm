---
"@pnpm/tarball-fetcher": patch
"@pnpm/git-fetcher": patch
"@pnpm/worker": patch
"pnpm": patch
---

When installing git-hosted dependencies, only pick the files that would be packed with the package [#7638](https://github.com/pnpm/pnpm/pull/7638).
