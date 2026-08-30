---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

Fixed `pnpm deploy --prod` failing when an excluded dev dependency was also declared as an optional peer dependency [pnpm/pnpm#14302](https://github.com/pnpm/pnpm/issues/14302).
