---
"@pnpm/deps.inspection.outdated": patch
"pnpm": patch
---

`pnpm outdated` no longer checks the registry for dependencies that are resolved from local `link:`, `file:`, or `workspace:` references in the lockfile [#12827](https://github.com/pnpm/pnpm/issues/12827).
