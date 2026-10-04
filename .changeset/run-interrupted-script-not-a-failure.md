---
"pacquet": patch
---

`pnpm run` no longer prints `[ELIFECYCLE] Command failed ...` after Ctrl+C ends the script. The interrupt is the user's doing, and the status it leaves is the shell's rather than the script's: on Windows, `cmd` ends with `-1073741510` and PowerShell with `1`; on Unix, the shell re-raises `SIGINT`. pnpm still ends the way the script's shell did [#16579](https://github.com/pnpm/pnpm/issues/16579).
