---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Speed up installs after compatible catalog or direct dependency range changes by retaining the locked version without resolving the dependency graph again.
