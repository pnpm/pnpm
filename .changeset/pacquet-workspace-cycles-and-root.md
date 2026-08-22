---
"pacquet": minor
---

`pnpm` now supports three workspace settings in its Rust engine:

- `includeWorkspaceRoot` (and the universal `--include-workspace-root` / `--no-include-workspace-root` flags) keeps the workspace root project in a recursive `run`, `exec`, `add`, or `test`, which otherwise leave it out.
- `ignoreWorkspaceCycles` and `disallowWorkspaceCycles` control the report an install makes when workspace projects depend on each other in a cycle: it is a warning by default, an `ERR_PNPM_DISALLOW_WORKSPACE_CYCLES` error under `disallowWorkspaceCycles`, and silent under `ignoreWorkspaceCycles` [#12042](https://github.com/pnpm/pnpm/issues/12042).
