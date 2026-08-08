---
"pacquet": patch
---

The pnpm v12 package now includes Corepack-compatible `bin/pnpm.mjs` and `bin/pnpx.mjs` entrypoints so Corepack can launch the pacquet wrapper without relying on the skipped `preinstall` relink step.
