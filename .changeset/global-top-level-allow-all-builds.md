---
"pacquet": patch
---

Commands that act on global packages, such as `pnpm add -g`, now honor `dangerouslyAllowAllBuilds` set at the top level of the global `config.yaml`. pnpm ignored it for global packages unless it was set under `globalPackagesConfig`.
