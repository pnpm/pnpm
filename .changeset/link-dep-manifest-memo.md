---
"pacquet": patch
---

Sped up linking bins in large workspaces. pnpm now reuses a workspace package's `package.json` across the projects that depend on it when linking their bins.
