## 1100.3.0

### Minor Changes

- `pmOnFail` and `lockfileDir` in `pnpm-workspace.yaml` now stay in force when another setting in that file cannot be read. pnpm used to switch to the pinned version despite `pmOnFail: ignore` in that case.

### Patch Changes

- Updated dependencies:
  - @pnpm/types@1102.1.2
