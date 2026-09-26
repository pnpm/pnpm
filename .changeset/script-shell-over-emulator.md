---
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and lifecycle scripts use the configured `scriptShell` on Windows, including Git Bash, when `shellEmulator` is also enabled. `shellEmulator` still runs scripts when `scriptShell` is not set. Extra arguments passed to `pnpm run` are quoted for the shell that runs the script, so a Windows path stays intact [#14719](https://github.com/pnpm/pnpm/issues/14719).
