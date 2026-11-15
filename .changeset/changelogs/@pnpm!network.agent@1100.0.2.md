## 1100.0.2

### Patch Changes

- `@pnpm/network.config` now exports `isNoProxy`, and `pickSettingByUrl` accepts a `skipFalsyValues` option. `@pnpm/network.agent` and `@pnpm/network.fetch` use them for proxy exclusions and client certificate lookup [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).

- Updated dependencies:
  - @pnpm/network.config@1100.0.1
