---
"@pnpm/worker": patch
"pnpm": patch
---

`pnpm install` now completes after downloading a Node.js runtime specified by `devEngines.runtime` when pnpm runs on Node.js 24.4.x. [#14667](https://github.com/pnpm/pnpm/issues/14667).
