---
"@pnpm/package-store": minor
"@pnpm/get-context": minor
"pnpm": minor
---

Added project registry for global virtual store prune support.

Projects using the store are now registered via symlinks in `{storeDir}/v10/projects/`. This enables `pnpm store prune` to track which packages are still in use by active projects and safely remove unused packages from the global virtual store.
