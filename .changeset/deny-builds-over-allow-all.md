---
"pacquet": patch
"@pnpm/napi": patch
---

A package set to `false` in `allowBuilds` no longer runs its build scripts when `dangerouslyAllowAllBuilds` is `true`. The two settings together now allow every build except the denied ones; before, `dangerouslyAllowAllBuilds` ignored the denials.
