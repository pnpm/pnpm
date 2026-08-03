## 1100.0.15

### Patch Changes

- Dependency resolution is faster: package metadata is now filtered once per packument instead of once per dependency edge when `minimumReleaseAge` is active, and parsed semver versions and ranges are reused instead of re-parsed on every comparison.

- Updated dependencies:
  - @pnpm/resolving.registry.types@1100.1.9
