---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed nondeterministic lockfile output that made `pnpm dedupe --check` fail intermittently in CI. When a locked peer provider was pinned for a dependency that has no child dependencies of its own, the pinned provider leaked into the shared parent scope, so siblings resolved after it could pick up an optional peer they should not see. Which siblings were affected depended on resolution order, which varies with network timing.
