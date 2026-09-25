---
"pacquet": patch
---

On Windows, installing pnpm with npm inside a project now writes `node_modules/.bin` shims that run `pnpm.exe`. A global install with `npm install --location=global` now gets the same shims as `npm install -g` [#15688](https://github.com/pnpm/pnpm/issues/15688).
