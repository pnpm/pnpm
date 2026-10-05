---
"@pnpm/default-reporter": patch
"pnpm": patch
---

The update notification now suggests the [standalone install script](https://pnpm.io/installation) instead of `pnpm self-update` when `PNPM_HOME` is not set, such as when another package manager installed pnpm. `pnpm self-update` installs into `PNPM_HOME`, so in that case it cannot replace the pnpm in use.
