---
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

On Windows, `pnpm run` now passes the arguments after the script name to the script unchanged. Before, `cmd` expanded `%VAR%` in them and backslashes arrived doubled [#16257](https://github.com/pnpm/pnpm/issues/16257).
