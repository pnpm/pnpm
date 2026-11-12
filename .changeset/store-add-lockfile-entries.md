---
"pacquet": minor
---

`pnpm store add` run in a project now prepares downloaded tarballs for every lockfile entry that pins them. Besides registry packages, `pnpm install --offline --frozen-lockfile` can then install dependencies on a tarball URL and dependencies on a git host such as GitHub whose lockfile entry records an `integrity`.
