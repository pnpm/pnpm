---
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pnpm": patch
"pacquet": patch
---

Prevented `minimumReleaseAge` from replacing `latest` with a SemVer-greater version than the registry tag target [#13034](https://github.com/pnpm/pnpm/issues/13034).
