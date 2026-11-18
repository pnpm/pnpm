## 1100.3.10

### Patch Changes

- `pnpm lane <lane> --filter <selector>` now rejects partial moves of a `versioning.fixed` group with `ERR_PNPM_VERSIONING_CONFLICTING_CONFIG` before writing `pnpm-workspace.yaml`.

- Updated dependencies:
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
