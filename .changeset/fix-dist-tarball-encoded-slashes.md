---
"@pnpm/resolving.tarball-url": patch
"pnpm": patch
"pacquet": patch
---

Fix an issue where scoped packages using percent-encoded slashes (`%2f` or `%2F`) in their registry tarball URLs could have their URLs incorrectly omitted from the lockfile, subsequently causing 404 errors during installation on registries that require percent-encoding (e.g. GitHub Enterprise Server).
