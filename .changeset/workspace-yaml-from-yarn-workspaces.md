---
"pacquet": minor
---

Install now creates `pnpm-workspace.yaml` from the `workspaces` field in the root `package.json` when no workspace manifest exists, so a repository converted from Yarn or npm links its projects on the first install instead of warning about the unsupported field [pnpm/pnpm#2255](https://github.com/pnpm/pnpm/issues/2255). An existing `pnpm-workspace.yaml` always wins, and `--ignore-workspace` keeps the project standalone.
