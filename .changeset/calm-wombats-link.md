---
"@pnpm/installing.context": patch
"pnpm": patch
---

Prevent installs through a symlinked `node_modules` directory from rewriting the target checkout [pnpm/pnpm#14286](https://github.com/pnpm/pnpm/issues/14286).
