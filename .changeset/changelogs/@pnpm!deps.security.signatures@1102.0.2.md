## 1102.0.2

### Patch Changes

- `pnpm self-update`, `pnpm with`, and automatic package-manager version switching no longer wait through registry retry delays when a configured registry has no signatures and `registry.npmjs.org` is unavailable [#14483](https://github.com/pnpm/pnpm/issues/14483).

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/network.fetch@1100.1.15
