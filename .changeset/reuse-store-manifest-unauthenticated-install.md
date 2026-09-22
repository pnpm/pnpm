---
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` reuses a package already present in the store when an existing lockfile entry satisfies the dependency, avoiding registry requests that fail without authorization [pnpm/pnpm#2522](https://github.com/pnpm/pnpm/issues/2522).
