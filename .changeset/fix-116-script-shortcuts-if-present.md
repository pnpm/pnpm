---
"@pnpm/exec.commands": patch
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

`pnpm --if-present start`, `pnpm run --if-present start`, and `pnpm restart --if-present` now exit with code 0 when a project has neither a `start` script nor a `server.js` file [zkochan/pnpm-tasks#116](https://github.com/zkochan/pnpm-tasks/issues/116).
