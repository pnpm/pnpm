---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Speed up installs after compatible catalog range changes by retaining the locked version without resolving the dependency graph again.
