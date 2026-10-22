---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now updates the project's pnpm pin when Homebrew installed pnpm or Corepack runs it. The global pnpm is left alone [#17001](https://github.com/pnpm/pnpm/issues/17001).
