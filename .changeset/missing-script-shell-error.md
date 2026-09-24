---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

When the configured `scriptShell` does not exist, running a script now fails with an error that names the shell, instead of only an exit code or the package directory [#7562](https://github.com/pnpm/pnpm/issues/7562).
