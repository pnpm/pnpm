---
"pacquet": patch
---

`pnpm run` no longer prints `[ELIFECYCLE] Command failed with exit code N.` to stderr when a script fails under `--reporter=silent`, `--silent`, or `--loglevel=silent`.
