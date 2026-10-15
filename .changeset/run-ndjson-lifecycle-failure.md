---
"pacquet": patch
---

`pnpm run` now reports a failed script as a JSON record under `--reporter=ndjson`. It printed a plain `[ELIFECYCLE]` line to stderr.
