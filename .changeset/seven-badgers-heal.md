---
"@pnpm/lifecycle": patch
"@pnpm/core": patch
"@pnpm/headless": patch
---

Check `neverBuiltDependencies` before a package runs it's lifecyle hooks. Fixes [#5407](https://github.com/pnpm/pnpm/issues/5407)
