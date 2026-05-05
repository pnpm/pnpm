---
"@pnpm/config.reader": patch
"pnpm": patch
---

Print a warning when settings that are not allowed in the global config file (e.g. `nodeLinker`, `hoistPattern`) are present in `config.yaml` and silently ignored. Previously these settings were dropped without any feedback, leaving users unsure why their global configuration had no effect.
