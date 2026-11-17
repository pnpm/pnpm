---
"@pnpm/pnpr": patch
---

pnpr now serves a version published upstream after it cached the package's packument. A request for a version, its tarball, or its manifest that the cached packument does not list refetches the packument, at most once a minute per package. Before, pnpr answered `404` until `packumentTtl` expired [#18180](https://github.com/pnpm/pnpm/issues/18180).
