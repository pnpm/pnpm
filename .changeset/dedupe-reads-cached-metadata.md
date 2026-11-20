---
"pacquet": patch
---

`pnpm dedupe --check` no longer requests the registry metadata of locked versions that the cache already holds. `pnpm dedupe` skips those requests too when `lockfile.format` is `v13`, which records no `deprecated` notices. Both were slow against a registry that sends no `ETag` or `Last-Modified`, such as Azure Artifacts, where each request downloaded the full package metadata [#17758](https://github.com/pnpm/pnpm/issues/17758).
