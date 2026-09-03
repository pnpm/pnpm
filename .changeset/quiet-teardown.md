---
"pacquet": patch
---

Sped up installs in large workspaces. Freeing the lockfile and manifest data held in memory no longer delays the end of the install, and lockfile keys are rendered once instead of twice when saving [#14352](https://github.com/pnpm/pnpm/issues/14352).
