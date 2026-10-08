---
"pacquet": patch
"@pnpm/napi": patch
---

`pnpm rebuild` no longer removes and recreates `node_modules` when the settings recorded in `node_modules/.modules.yaml` differ from the current configuration. The rebuild runs the build scripts against the installed packages as they are.
