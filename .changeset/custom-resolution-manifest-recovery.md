---
"pacquet": patch
---

Fixed a package resolved by a `resolvers` pnpmfile hook installing without its own dependencies. This happened when the hook returned no `manifest` and a `fetchers` hook handled the resolution [#15552](https://github.com/pnpm/pnpm/issues/15552).
