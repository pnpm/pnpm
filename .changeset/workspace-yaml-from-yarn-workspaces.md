---
"pacquet": minor
---

`pnpm install` now creates `pnpm-workspace.yaml` from the `workspaces` field of the root `package.json` when the repository has no `pnpm-workspace.yaml`. The projects the field lists are linked on that same install. An existing `pnpm-workspace.yaml` is never changed, and `--ignore-workspace` creates no file [#2255](https://github.com/pnpm/pnpm/issues/2255).
