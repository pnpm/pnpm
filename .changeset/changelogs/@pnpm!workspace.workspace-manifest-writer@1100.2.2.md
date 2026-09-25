## 1100.2.2

### Patch Changes

- pnpm now treats a missing global `config.yaml`, `auth.ini`, or other optional config file as absent in Node.js-compatible runtimes such as StackBlitz WebContainers. Commands such as `pnpm --version` failed there with `ENOENT` [#14030](https://github.com/pnpm/pnpm/issues/14030).

- Updated dependencies:
  - @pnpm/building.policy@1100.1.3
  - @pnpm/config.parse-overrides@1100.1.6
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.workspace-manifest-reader@1100.2.0
