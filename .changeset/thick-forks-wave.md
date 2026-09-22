---
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

Bin linking leaves workspace and linked dependency files outside node_modules unchanged. Already executable bin files no longer receive redundant permission changes.
