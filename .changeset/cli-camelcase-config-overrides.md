---
"pacquet": patch
---

Validate `--config.<camelCase>` CLI setting values, route `--config.storeDir` and `--config.stateDir` through the global `--store-dir` and `--state-dir` flags, and accept empty `--state-dir=` values to reset to the default state directory, matching `--store-dir=`.
