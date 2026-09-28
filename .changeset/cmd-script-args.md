---
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

On Windows, `pnpm run` now passes the arguments after the script name to the script as typed. Before, `cmd` expanded `%VAR%` in them and backslashes arrived doubled. Line breaks still arrive as the two characters `\n`, because `cmd` cannot pass them. The command line pnpm prints for the script quotes the arguments the same way on every platform [#16257](https://github.com/pnpm/pnpm/issues/16257).
