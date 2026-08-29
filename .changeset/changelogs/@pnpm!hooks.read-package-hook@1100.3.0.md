## 1100.3.0

### Minor Changes

- `pnpm update` no longer moves the range a project declares for a dependency that `overrides` also lists, even when the override repeats that range verbatim. Previously the updated `package.json` disagreed with the lockfile, so the next `pnpm install --frozen-lockfile` failed with a specifier mismatch [#14224](https://github.com/pnpm/pnpm/issues/14224).

### Patch Changes

- Updated dependencies:
  - @pnpm/types@1102.1.0
