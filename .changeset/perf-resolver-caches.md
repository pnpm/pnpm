---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pnpm": patch
---

Dependency resolution is ~13% faster: package metadata is now filtered once per packument instead of once per dependency edge when `minimumReleaseAge` is active, and parsed semver versions/ranges are cached instead of re-parsed on every comparison.
