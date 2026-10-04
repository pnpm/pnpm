---
"pacquet": patch
---

`pnpm run` no longer prints `[ELIFECYCLE] Command failed ...` after Ctrl+C ends the script. pnpm still exits the way the script's shell did: on Windows with the shell's exit code (`cmd` reports `-1073741510`, PowerShell `1`), on Unix by re-raising `SIGINT` [#16579](https://github.com/pnpm/pnpm/issues/16579).
