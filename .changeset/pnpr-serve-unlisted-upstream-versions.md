---
"@pnpm/pnpr": patch
---

An upstream registry now serves a version published after pnpr cached the package's packument. A request for a version, its tarball, or its manifest that the cached packument does not list refetches the packument if it was fetched more than a minute ago. Before, pnpr answered `404` until `packumentTtl` expired [#18180](https://github.com/pnpm/pnpm/issues/18180).
