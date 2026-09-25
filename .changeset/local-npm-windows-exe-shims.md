---
"@pnpm/exe": patch
"pnpm": patch
---

On Windows, installing `@pnpm/exe` with npm inside a project now writes `node_modules/.bin` shims that run the standalone executable [#15688](https://github.com/pnpm/pnpm/issues/15688).
