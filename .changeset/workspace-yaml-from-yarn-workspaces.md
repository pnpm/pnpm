---
"pacquet": minor
---

`pnpm install` now creates `pnpm-workspace.yaml` from the `workspaces` field of the root `package.json` when the repository has no `pnpm-workspace.yaml`. The projects the field lists are linked on that same install. An existing `pnpm-workspace.yaml` is never changed. With `--ignore-workspace`, no file is created. If the `workspaces` field later differs from `packages` in `pnpm-workspace.yaml`, pnpm prints a warning [#2255](https://github.com/pnpm/pnpm/issues/2255).
