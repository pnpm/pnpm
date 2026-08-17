## 1101.1.2

### Patch Changes

- `pnpm deploy` injects workspace dependencies again, so the deploy directory is self-contained instead of symlinking back into the source workspace [#13754](https://github.com/pnpm/pnpm/issues/13754). Enabling `injectWorkspacePackages` with `dedupeInjectedDeps` disabled now also rewrites already-linked workspace dependencies to injected copies.

- A lockfile entry whose resolution is unchanged no longer loses its recorded `deprecated` marker when a registry serves the package's metadata inconsistently — re-resolving to the same version keeps the deprecation instead of silently dropping the line [#13846](https://github.com/pnpm/pnpm/issues/13846).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/deps.graph-hasher@1100.2.17
  - @pnpm/error@1100.1.2
  - @pnpm/fetching.pick-fetcher@1100.1.7
  - @pnpm/lockfile.preferred-versions@1100.0.29
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/patching.config@1100.1.1
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/pkg-manifest.utils@1100.4.0
  - @pnpm/resolving.npm-resolver@1103.2.1
  - @pnpm/store.controller-types@1101.1.1
