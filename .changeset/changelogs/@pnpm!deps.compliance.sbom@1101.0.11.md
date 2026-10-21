## 1101.0.11

### Patch Changes

- `pnpm sbom` now reads project lockfiles in workspaces with `sharedWorkspaceLockfile: false`, including filtered and split output.

- `pnpm sbom` now reports the repository of a package whose `repository` field is a `gist:` shorthand.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.package-is-installable@1100.2.4
  - @pnpm/lockfile.detect-dep-types@1100.0.28
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/lockfile.walker@1100.0.28
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.index@1101.0.3
  - @pnpm/store.pkg-finder@1100.0.41
  - @pnpm/types@1102.1.2
