---
"@pnpm/installing.deps-resolver": patch
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

`packageExtensions` and `overrides` entries with a ranged selector (such as `@<X` or `@*`) no longer match a local directory dependency that has no `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).
