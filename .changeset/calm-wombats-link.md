---
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Prevent installs through a symlinked `node_modules` directory from rewriting the target checkout, and make `pnpm add --lockfile-only` skip dependency linking [pnpm/pnpm#14286](https://github.com/pnpm/pnpm/issues/14286).
