---
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
---

Fixed pnpm failing to start under asynchronous Node.js module loaders when no `.pnpmfile.mjs` exists [pnpm/pnpm#11701](https://github.com/pnpm/pnpm/issues/11701).
