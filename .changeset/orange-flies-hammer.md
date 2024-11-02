---
"@pnpm/core": patch
---

Fix for headless install crashing when modules directory disabled (`enable-modules-dir` set to `false`) and patched dependencies are present [#8727](https://github.com/pnpm/pnpm/pull/8727).
