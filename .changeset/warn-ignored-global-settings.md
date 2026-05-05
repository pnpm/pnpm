---
"@pnpm/config.reader": patch
"pnpm": patch
---

Print a warning when settings that are not allowed in the global config file (e.g. `nodeLinker`, `hoistPattern`) are present in `config.yaml` and silently ignored. Previously these settings were dropped without any feedback, leaving users unsure why their global configuration had no effect. The warning suggests moving those settings to a project-level `pnpm-workspace.yaml`, or sharing them across projects via [config dependencies](https://pnpm.io/11.x/config-dependencies).
