---
"@pnpm/resolve-dependencies": patch
"supi": patch
---

Ignore non-array bundle[d]Dependencies fields. Fixes a regression caused by https://github.com/pnpm/pnpm/commit/5322cf9b39f637536aa4775aa64dd4e9a4156d8a
