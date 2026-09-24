---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`packageExtensions` ranged selectors (such as `@<X` or `@*`) no longer match dependencies that ship without a `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).
