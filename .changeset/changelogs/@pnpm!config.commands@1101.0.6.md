## 1101.0.6

### Patch Changes

- pnpm now treats a missing global `config.yaml`, `auth.ini`, or other optional config file as absent in Node.js-compatible runtimes such as StackBlitz WebContainers. Commands such as `pnpm --version` failed there with `ENOENT` [#14030](https://github.com/pnpm/pnpm/issues/14030).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/error@1100.2.0
  - @pnpm/object.property-path@1100.1.7
  - @pnpm/workspace.workspace-manifest-writer@1100.2.2
