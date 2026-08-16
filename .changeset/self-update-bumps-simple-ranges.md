---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now bumps `devEngines.packageManager.version` when it is a simple range (`^`/`~`) that the new version still satisfies, keeping the same operator — matching `pnpm update` and `pnpm runtime set`. Complex ranges such as `>=8.0.0` are still left unchanged.
