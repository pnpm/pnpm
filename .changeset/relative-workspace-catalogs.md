---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` failing to resolve catalog dependencies in injected workspace packages referenced by relative paths [pnpm/pnpm#17280](https://github.com/pnpm/pnpm/issues/17280).
