---
"@pnpm/headless": patch
"@pnpm/build-modules": patch
"@pnpm/core": patch
"pnpm": patch
---

Don't read a package from side-effects cache if it isn't allowed to be built [#9042](https://github.com/pnpm/pnpm/issues/9042).
