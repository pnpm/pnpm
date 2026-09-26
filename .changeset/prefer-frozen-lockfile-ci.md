---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`prefer-frozen-lockfile=true` in configuration now correctly keeps the lockfile frozen during installation on CI [pnpm/pnpm#9072](https://github.com/pnpm/pnpm/pull/9072).
