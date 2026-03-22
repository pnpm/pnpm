---
"@pnpm/workspace.range-resolver": minor
"@pnpm/releasing.exportable-manifest": minor
"@pnpm/resolving.npm-resolver": minor
pnpm: minor
---

Support bare `workspace:` protocol without version specifier. It is now treated as `workspace:*` and resolves to the concrete version during publish [#10436](https://github.com/pnpm/pnpm/pull/10436).
