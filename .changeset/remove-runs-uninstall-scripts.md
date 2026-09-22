---
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked. A failure in either stage aborts the removal. `postuninstall` runs after unlinking completes. The `ignoreScripts` setting and `--lockfile-only` skip all three stages [#3276](https://github.com/pnpm/pnpm/issues/3276).
