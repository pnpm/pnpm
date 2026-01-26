---
"@pnpm/resolve-workspace-range": minor
"@pnpm/exportable-manifest": minor
"@pnpm/npm-resolver": minor
pnpm: minor
---

Support bare `workspace:` protocol without version specifier. It is now treated as `workspace:*` and resolves to the concrete version during publish [#10436](https://github.com/pnpm/pnpm/pull/10436).
