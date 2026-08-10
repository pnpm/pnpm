---
"@pnpm/config.commands": minor
"pnpm": minor
---

`pnpm config set` refuses to write a setting to a project's `pnpm-workspace.yaml` that pnpm does not read from there, rather than leaving a key in the file that does nothing. Those settings are `configDir`, `pnpmHomeDir`, `stateDir` and the others that name machine-level state. The command fails with `ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING` and names where the setting does belong. `pnpm config delete` still clears one that a file already carries, in whichever spelling it uses [#13629](https://github.com/pnpm/pnpm/issues/13629).
