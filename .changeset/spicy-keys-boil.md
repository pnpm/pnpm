---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

Explicitly throw an error when user attempts to run `publish` or `pack` when `bundledDependencies` is set but `node-linker` isn't `hoisted`.
