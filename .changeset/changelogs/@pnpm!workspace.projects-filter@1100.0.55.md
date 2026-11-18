## 1100.0.55

### Patch Changes

- Combining `--filter` and `--filter-prod` with exclusion selectors such as `!pkg` now evaluates all selectors together in command-line order. An exclusion passed on one flag while inclusions were passed on the other no longer selects every workspace project.

- Updated dependencies:
  - @pnpm/workspace.projects-graph@1100.0.49
  - @pnpm/workspace.projects-reader@1101.1.8
  - @pnpm/workspace.workspace-manifest-reader@1100.3.3
