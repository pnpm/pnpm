---
"pacquet": patch
---

`ignorePnpmfile` can now be set in `pnpm-workspace.yaml` and read from `PNPM_CONFIG_IGNORE_PNPMFILE`, not only passed as `--ignore-pnpmfile`, so a project or a machine can turn pnpmfile hooks off once instead of adding the flag to every command. The flag still applies on top. As in pnpm, the global `config.yaml` cannot set it: a pnpmfile belongs to the project that ships it.
