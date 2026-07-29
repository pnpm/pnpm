---
"pacquet": patch
---

A `pnpm-workspace.yaml` that declares a package pattern whose directory does not exist yet — `packages/*` before the first package is created, say — no longer fails every command with `ERR_PNPM_WORKSPACE_WALK_ERROR`. The pattern now matches no projects, as it does in the JavaScript implementation [#13296](https://github.com/pnpm/pnpm/issues/13296).
