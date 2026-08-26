---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Reduced registry metadata requests during dependency resolution by reusing cached metadata when lockfile preferences prove that no uncached version can win [pnpm/pnpm#13976](https://github.com/pnpm/pnpm/issues/13976).
