---
"@pnpm/types": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/pkg-manifest.utils": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update` keeps the explicit `=` operator of an exact version pin: a dependency saved as `=3.5.1` now updates to `=3.5.2` instead of the bare `3.5.2`. See pnpm/pnpm#13168.
