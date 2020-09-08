---
"supi": patch
---

In some rare cases, `pnpm install --no-prefer-frozen-lockfile` didn't link the direct dependencies to the root `node_modules`. This was happening when the direct dependency was also resolving some peer dependencies.
