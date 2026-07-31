## 1100.0.30

### Patch Changes

- Workspace dependencies declared with a relative path (e.g. `"foo": "workspace:../foo"`) are no longer silently dropped from the workspace projects graph, so `--filter` selection and the topological order of recursive commands take them into account.

- Updated dependencies:
  - @pnpm/resolving.npm-resolver@1103.0.0
  - @pnpm/types@1101.8.0
