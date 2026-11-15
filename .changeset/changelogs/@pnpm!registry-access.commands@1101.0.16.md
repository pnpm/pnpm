## 1101.0.16

### Patch Changes

- `pnpm dist-tag` now offers browser-based authentication when the account's two-factor authentication uses a security key. Before, it asked for a one-time password in that case [#17832](https://github.com/pnpm/pnpm/issues/17832).

- Fixed `pnpm deprecate`, `pnpm undeprecate`, and `pnpm unpublish` failing with revision conflicts when a registry cache serves stale package metadata.

- `pnpm unpublish` now offers browser-based authentication when the account's two-factor authentication uses a security key. Before, it asked for a one-time password in that case [#17832](https://github.com/pnpm/pnpm/issues/17832).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.35
  - @pnpm/config.reader@1102.3.9
  - @pnpm/network.fetch@1100.1.25
  - @pnpm/registry-access.client@1100.1.27
