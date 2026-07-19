---
"pacquet": minor
---

Repeat installs now reconcile the existing `node_modules` the way the TypeScript CLI does: direct dependencies removed from the lockfile lose their links and bin shims, hoisted aliases of removed packages are unlinked and rehoisted, a hand-deleted package is detected and re-installed even when the lockfile is otherwise up to date, and `pnpm add` / `pnpm remove` fail with `ERR_PNPM_HOIST_PATTERN_DIFF`-family errors instead of silently recreating a modules directory whose layout settings drifted. Dev-only installs also no longer delete `node_modules/.pnpm/lock.yaml`.
