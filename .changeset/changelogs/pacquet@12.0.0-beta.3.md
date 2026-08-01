## 12.0.0-beta.3

### Patch Changes

- With `excludeLinksFromLockfile` enabled, a `link:` dependency pointing inside the workspace is no longer treated as an external link when it resolves a peer dependency, so the peer suffixes it produces stay identical to an install with the setting off. Injected (`file:`) workspace dependencies are no longer affected by the setting either [#13556](https://github.com/pnpm/pnpm/issues/13556).

- A registry dependency is now always recorded in `pnpm-lock.yaml` with an integrity hash, including under `--lockfile-only`. Packages from a registry that publishes no subresource-integrity metadata — `https://node-registry.bit.cloud/`, for one — were recorded without one, so the next `pnpm install --frozen-lockfile` failed with `ERR_PNPM_MISSING_TARBALL_INTEGRITY` [#13547](https://github.com/pnpm/pnpm/issues/13547).

- Dependency resolution is faster: package metadata is now filtered once per packument instead of once per dependency edge when `minimumReleaseAge` is active, and parsed semver versions and ranges are reused instead of re-parsed on every comparison.

- Reduced memory use when resolving peer-heavy dependency graphs and prevented nested hoisted graphs from expanding into excessive dependency paths.
