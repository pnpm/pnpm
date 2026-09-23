---
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm add`, `pnpm update`, and `pnpm remove` now save `package.json` when a project's own lifecycle script, such as the workspace root's `postinstall`, fails after the lockfile is written. The command still exits with the script's error. Previously the lockfile recorded the change but `package.json` did not [#8627](https://github.com/pnpm/pnpm/issues/8627).
