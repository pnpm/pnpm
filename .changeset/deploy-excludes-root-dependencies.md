---
"@pnpm/installing.commands": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` no longer installs the dependencies of the workspace root project into the deploy directory [#6437](https://github.com/pnpm/pnpm/issues/6437).
