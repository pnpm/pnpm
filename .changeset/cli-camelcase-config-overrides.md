---
"pacquet": patch
---

Validate `--config.<camelCase>` CLI setting values and route `--config.storeDir` and `--config.stateDir` through the global `--store-dir` and `--state-dir` flags, matching their kebab-case spellings.
