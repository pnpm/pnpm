---
"@pnpm/deps.status": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.
