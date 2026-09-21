---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` no longer triggers an install when running scripts in a read-only deployed filesystem [#11617](https://github.com/pnpm/pnpm/issues/11617).
