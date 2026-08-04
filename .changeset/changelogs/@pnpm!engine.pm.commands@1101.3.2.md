## 1101.3.2

### Patch Changes

- The env lockfile no longer pins `@pnpm/exe` alongside `pnpm` when the wanted pnpm version is 12 or newer. From v12 the unscoped `pnpm` package is itself the native executable, so `@pnpm/exe` is not published for it and resolving it would fail. The engine identity check now verifies the native binary through whichever package ships it.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.25
  - @pnpm/building.policy@1100.0.18
  - @pnpm/cli.meta@1100.0.14
  - @pnpm/cli.utils@1101.0.22
  - @pnpm/config.pick-registry-for-package@1100.1.0
  - @pnpm/config.reader@1101.16.0
  - @pnpm/config.version-policy@1100.1.12
  - @pnpm/deps.graph-hasher@1100.2.15
  - @pnpm/deps.security.signatures@1101.2.10
  - @pnpm/error@1100.1.1
  - @pnpm/global.commands@1100.0.43
  - @pnpm/global.packages@1100.0.16
  - @pnpm/installing.client@1100.3.2
  - @pnpm/installing.deps-restorer@1102.3.0
  - @pnpm/installing.env-installer@1102.0.15
  - @pnpm/lockfile.fs@1100.2.0
  - @pnpm/lockfile.types@1100.0.19
  - @pnpm/network.auth-header@1101.1.9
  - @pnpm/pkg-manifest.utils@1100.3.1
  - @pnpm/resolving.npm-resolver@1103.1.0
  - @pnpm/store.connection-manager@1100.3.15
  - @pnpm/store.controller@1102.0.11
  - @pnpm/types@1101.9.0
  - @pnpm/workspace.project-manifest-reader@1100.0.23
