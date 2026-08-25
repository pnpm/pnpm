---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now rewrites a simple `devEngines.packageManager.version` range (`^`/`~`) to the newly installed version, keeping the operator — matching how `pnpm update` and `pnpm runtime set` rewrite ranges. Complex ranges such as `>=8.0.0` that the new version satisfies are still left unchanged [#13935](https://github.com/pnpm/pnpm/issues/13935).
