## 1102.1.1

### Patch Changes

- The JavaScript pnpm can again switch to the pnpm version a project pins in `packageManager` on hosts where the native pnpm build ships no binary, such as Alpine Linux with pnpm 10 or an Intel Mac with pnpm 11 [#13622](https://github.com/pnpm/pnpm/issues/13622).

  When the pnpm build being switched to is native and ships no binary for the host, pnpm now names the host target it lacks. pnpm reported that the binary was missing from `pnpm-lock.yaml`.

- Fixed standalone installations to preserve the bundled `node-gyp` files used to build native dependencies.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.30
  - @pnpm/building.policy@1100.1.0
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/config.version-policy@1100.2.3
  - @pnpm/deps.graph-hasher@1100.3.1
  - @pnpm/deps.security.signatures@1102.0.2
  - @pnpm/error@1100.1.4
  - @pnpm/global.commands@1102.0.0
  - @pnpm/global.packages@1101.1.1
  - @pnpm/installing.client@1100.3.8
  - @pnpm/installing.deps-restorer@1103.1.1
  - @pnpm/installing.env-installer@1103.0.3
  - @pnpm/lockfile.fs@1100.2.6
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/resolving.npm-resolver@1104.1.1
  - @pnpm/store.connection-manager@1101.1.1
  - @pnpm/store.controller@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.0.27
