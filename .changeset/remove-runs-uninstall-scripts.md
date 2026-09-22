---
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before the removed dependencies are unlinked, and a failure in either leaves the project untouched. `postuninstall` runs after they are unlinked. The `ignoreScripts` setting and `--lockfile-only` skip all three. The scripts of the removed package itself still do not run [#3276](https://github.com/pnpm/pnpm/issues/3276).
