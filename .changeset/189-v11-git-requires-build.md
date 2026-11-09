---
"@pnpm/worker": patch
"pnpm": patch
---

`pnpm install` now builds a git dependency with install scripts that an older pnpm release marked in a shared store as not requiring a build [pnpm/tasks#189](https://github.com/pnpm/tasks/issues/189).
