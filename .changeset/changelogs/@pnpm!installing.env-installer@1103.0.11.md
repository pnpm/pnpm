## 1103.0.11

### Patch Changes

- `pnpm install` and `pnpm add --config` now apply `minimumReleaseAge` when they resolve a config dependency. A config dependency range resolves to the newest version that is old enough, so a later clean `pnpm install --frozen-lockfile` accepts the lockfile [#16660](https://github.com/pnpm/pnpm/issues/16660).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.8
  - @pnpm/config.writer@1100.0.33
  - @pnpm/installing.deps-resolver@1102.2.8
  - @pnpm/resolving.npm-resolver@1104.2.6
  - @pnpm/store.controller@1102.2.5
