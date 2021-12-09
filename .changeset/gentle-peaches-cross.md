---
"@pnpm/core": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Installation should be finished before an error about bad/missing peer dependencies is printed and kills the process.
