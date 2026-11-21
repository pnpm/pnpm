---
"pacquet": patch
---

Sped up linking bins in large workspaces. pnpm now reads each workspace package's `package.json` once per install, not once for every project that depends on it.
