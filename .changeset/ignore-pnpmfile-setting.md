---
"pacquet": patch
---

`ignorePnpmfile` can now be set in configuration and read from `PNPM_CONFIG_IGNORE_PNPMFILE`, not only passed as `--ignore-pnpmfile`. A project or a machine can turn pnpmfile hooks off once instead of adding the flag to every command; the flag still applies on top.
