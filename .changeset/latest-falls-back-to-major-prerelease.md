---
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pnpm": patch
"pacquet": patch
---

When `minimumReleaseAge` hides the version that `latest` points to, pnpm now falls back to a prerelease of the same major before a stable version of an older major. For example, if `1.0.0` is too new, `latest` resolves to `1.0.0-beta.4` rather than `0.0.1`. A stable version of the same major is still preferred over a prerelease [#16388](https://github.com/pnpm/pnpm/issues/16388).
