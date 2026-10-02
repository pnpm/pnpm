---
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm dlx` with `--package` but no command now reports a clear error: `'pnpm dlx' requires a command to run`. Previously it installed the package and then crashed trying to run an empty command.
