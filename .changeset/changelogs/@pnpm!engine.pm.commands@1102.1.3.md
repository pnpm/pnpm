## 1102.1.3

### Patch Changes

- `pn`, `pnpx`, and `pnx` now run the pnpm installed alongside them. They used to look pnpm up on `PATH`. That failed when the directory holding them was not on `PATH`, and it silently handed the call to an unrelated pnpm when one came first there [#14803](https://github.com/pnpm/pnpm/issues/14803).

- `pnpm setup` now describes the displayed configuration changes as "the following configuration changes."

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.32
  - @pnpm/building.policy@1100.1.2
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/deps.graph-hasher@1100.3.3
  - @pnpm/deps.security.signatures@1102.0.4
  - @pnpm/global.commands@1102.0.2
  - @pnpm/global.packages@1101.1.3
  - @pnpm/installing.client@1100.3.10
  - @pnpm/installing.deps-restorer@1103.1.3
  - @pnpm/installing.env-installer@1103.0.5
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/resolving.npm-resolver@1104.2.0
  - @pnpm/store.connection-manager@1101.1.3
  - @pnpm/store.controller@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.0.29
