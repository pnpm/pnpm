---
"@pnpm/deps.inspection.peers-checker": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` and `pnpm peers check` now resolve peer dependencies declared by workspace packages from their consuming projects and report unmet peer dependencies [pnpm/pnpm#8150](https://github.com/pnpm/pnpm/issues/8150).
