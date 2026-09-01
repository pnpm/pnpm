---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Changed the lockfile policy error to suggest relaxing the policy only after expected changes fail a fresh resolution and the affected packages are trusted [#14411](https://github.com/pnpm/pnpm/issues/14411).
