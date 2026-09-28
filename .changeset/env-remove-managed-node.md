---
"@pnpm/config.reader": patch
"@pnpm/engine.runtime.commands": patch
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm env remove --global` deletes Node.js versions that pnpm installed into its own store, including when another tool installed pnpm [pnpm/pnpm#8357](https://github.com/pnpm/pnpm/issues/8357).
