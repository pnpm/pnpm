---
"@pnpm/resolve-dependencies": patch
pnpm: patch
---

Fix a case of installs not being deterministic and causing lockfile changes between repeat installs. When a dependency only declares `peerDependenciesMeta` and not `peerDependencies`, `dependencies`, or `optionalDependencies`, the dependency's peers were not considered deterministically before.
