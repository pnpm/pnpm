---
"@pnpm/installing.env-installer": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

The env lockfile no longer pins `@pnpm/exe` alongside `pnpm` when the wanted pnpm version is 12 or newer. From v12 the unscoped `pnpm` package is itself the native executable, so `@pnpm/exe` is not published for it and resolving it would fail. The engine identity check now verifies the native binary through whichever package ships it.
