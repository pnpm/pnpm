---
"@pnpm/exec.commands": patch
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

`pnpm --if-present start`, `pnpm run --if-present start`, and `pnpm restart --if-present` now exit with code 0 when a project has neither a `start` script nor a `server.js` file [#18073](https://github.com/pnpm/pnpm/pull/18073).
