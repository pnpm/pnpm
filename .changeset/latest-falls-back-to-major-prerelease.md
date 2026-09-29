---
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pnpm": patch
"pacquet": patch
---

When `minimumReleaseAge` hides the version that `latest` points to, pnpm now falls back to a prerelease of the same major before a stable version of an older major. A stable version of the same major is still preferred. Previously, while a new `1.0.0` was too new, `latest` fell back to an old `0.0.1` even though `1.0.0-beta.4` had been `latest` until then [#16388](https://github.com/pnpm/pnpm/issues/16388).
