---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install` resolves the `link:` and bare relative path dependencies of an injected workspace package from that package's directory. They were resolved from the lockfile directory, so the installed link could point outside the workspace [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).
