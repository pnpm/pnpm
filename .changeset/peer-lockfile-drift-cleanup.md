---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed lockfile drift after manifest edits by removing reuse-only transitive peer suffixes without losing valid peer-context deduplication.
