---
"pacquet": patch
---

`pnpm -r run` no longer garbles its live output when a script fails while other scripts are still running. The `[ELIFECYCLE]` lines that pnpm printed for the failed scripts shifted the lines it was redrawing.
