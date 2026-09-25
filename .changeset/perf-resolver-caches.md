---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pacquet": patch
"pnpm": patch
---

Dependency resolution is faster: package metadata is now filtered once per packument instead of once per dependency edge when `minimumReleaseAge` is active, and parsed semver versions and ranges are reused instead of re-parsed on every comparison.
