---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` preserves an injected workspace package's resolved dependencies when they differ from the workspace project's own dependencies. This also applies to packages with `publishConfig.directory` [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).
