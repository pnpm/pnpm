---
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm exec <command>` and `pnpm <command>` run from a subdirectory of a project now find the executables installed in the project's `node_modules/.bin`. The command still runs in the subdirectory. `PNPM_PACKAGE_NAME` names the project [#5068](https://github.com/pnpm/pnpm/issues/5068).
