---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

An override change is now absorbed by the fast lockfile update even when another, unchanged override uses the `catalog:` protocol. Previously any `catalog:`-valued override forced a full re-resolution whenever the override list changed, which could move unrelated packages in the lockfile (for example after `pnpm audit --fix` added an override).
