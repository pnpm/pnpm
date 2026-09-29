---
"pacquet": patch
---

`pnpm config set --location=project` refuses a machine-level setting such as `stateDir` or `scope` with `ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING`, which names where the setting belongs. `pnpm config delete` still clears such a key from a project's `pnpm-workspace.yaml`.
