---
"@pnpm/default-reporter": patch
"pnpm": patch
---

`pnpm add a-module-already-in-dev-deps` will show a message to notice the user that the package was not moved to "dependencies" [#926](https://github.com/pnpm/pnpm/issues/926).
