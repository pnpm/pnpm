---
"@pnpm/plugin-commands-config": patch
---

When both `pnpm-workspace.yaml` and `.npmrc` exist, `pnpm config set --location=project` now writes to `pnpm-workspace.yaml` (matching read priority) [#10072](https://github.com/pnpm/pnpm/issues/10072).
