---
"@pnpm/exec.commands": patch
"@pnpm/exec.pnpm-cli-runner": patch
"pnpm": patch
---

Honor `--silent` when `verifyDepsBeforeRun: install` auto-installs dependencies before `pnpm run` or `pnpm exec`, preventing install output from being written to stdout [#11636](https://github.com/pnpm/pnpm/issues/11636).
