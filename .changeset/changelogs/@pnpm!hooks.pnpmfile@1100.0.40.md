## 1100.0.40

### Patch Changes

- `updateConfig` and `afterAllResolved` hooks in `.pnpmfile.cjs` that return `undefined`, `null`, or a non-object value now fail with `ERR_PNPM_CONFIG_IS_UNDEFINED` and `ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN`.

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/hooks.types@1101.0.9
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/store.controller-types@1101.3.7
  - @pnpm/types@1102.1.4
