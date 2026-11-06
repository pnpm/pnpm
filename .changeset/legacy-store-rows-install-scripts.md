---
"@pnpm/worker": patch
"pnpm": patch
"pacquet": patch
---

Packages that an older pnpm version added to the store now run their install scripts. Before, pnpm could skip those scripts and hardlink the package's files to the store.
