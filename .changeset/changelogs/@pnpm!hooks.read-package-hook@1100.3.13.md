## 1100.3.13

### Patch Changes

- When multiple `overrides` selectors match a dependency, pnpm now picks the selector with the narrower version range regardless of the order in which the overrides are declared [#18071](https://github.com/pnpm/pnpm/pull/18071).

- Updated dependencies:
  - @pnpm/resolving.local-resolver@1101.2.10
  - @pnpm/types@1102.1.4
