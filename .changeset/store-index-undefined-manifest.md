---
"pacquet": patch
---

`pnpm store prune` no longer fails on store index entries that pnpm 11 wrote for git-hosted packages without a `package.json`. Entries that still cannot be read are kept and counted in the prune summary.
