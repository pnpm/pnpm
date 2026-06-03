---
"@pnpm/pnpr.client": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `optionalDependencies` being dropped when resolving through a `pnprServer`. The pnpr request now carries each project's optional dependencies (for both single-project and workspace installs), so the server resolves them like the local resolver does instead of producing a lockfile as if they did not exist.
