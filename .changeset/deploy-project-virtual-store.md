---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` now records its project-local virtual store setting in the deployment, so running scripts in a read-only deployed filesystem does not trigger an install [#11617](https://github.com/pnpm/pnpm/issues/11617).
