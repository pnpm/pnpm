---
"@pnpm/exec.commands": patch
"@pnpm/exec.pnpm-cli-runner": patch
"pacquet": patch
"pnpm": patch
---

`pnpm run` with `--loglevel=warn` or `--loglevel=error` (or the same `loglevel` setting) no longer prints the `$ <command>` line before a script, nor the summary of the install that `verifyDepsBeforeRun` runs first. Both are info-level output. In pnpm 11 the same now holds for `--loglevel=silent` [#8944](https://github.com/pnpm/pnpm/issues/8944).
