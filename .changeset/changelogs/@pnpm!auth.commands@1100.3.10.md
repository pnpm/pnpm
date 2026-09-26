## 1100.3.10

### Patch Changes

- pnpm now treats a missing global `config.yaml`, `auth.ini`, or other optional config file as absent in Node.js-compatible runtimes such as StackBlitz WebContainers. Commands such as `pnpm --version` failed there with `ENOENT` [#14030](https://github.com/pnpm/pnpm/issues/14030).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.reader@1102.3.0
  - @pnpm/error@1100.2.0
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/network.web-auth@1101.6.1
  - @pnpm/registry-access.client@1100.1.20
