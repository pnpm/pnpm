## 1100.0.39

### Patch Changes

- `pnpm install` now removes command shims from `node_modules/.bin` when a workspace dependency renames or removes an entry from its `bin` field [pnpm/tasks#211](https://github.com/pnpm/tasks/issues/211).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.19
  - @pnpm/pkg-manifest.reader@1100.0.24
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
