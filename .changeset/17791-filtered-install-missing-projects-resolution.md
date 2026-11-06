---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --filter` in a sparse checkout now writes the same lockfile as a full checkout. The dependencies of workspace projects that are missing from disk now count when pnpm resolves peer dependencies and marks packages as optional [pnpm/pnpm#17791](https://github.com/pnpm/pnpm/issues/17791).
