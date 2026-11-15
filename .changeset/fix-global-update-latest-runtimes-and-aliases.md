---
"@pnpm/global.commands": patch
"@pnpm/global.packages": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update -g --latest` now upgrades global runtimes (`node`, `deno`, and `bun`) and `npm:`-aliased global packages across major versions.
