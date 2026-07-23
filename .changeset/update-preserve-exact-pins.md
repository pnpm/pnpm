---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/pkg-manifest.utils": patch
"@pnpm/types": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm update` rewriting exact version pins that use the `=` operator (for example `=3.5.1`) to a caret range (`^3.5.1`) or a bare version (`3.5.1`). Exact pins are now preserved with the `=` operator. See pnpm/pnpm#12745 and pnpm/pnpm#13168.
