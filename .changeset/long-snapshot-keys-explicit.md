---
"pacquet": patch
---

Fixed writing lockfiles with dependency paths longer than 1024 characters (long peer suffixes in large workspaces): such keys are now emitted in explicit `? <key>` form, matching the TypeScript CLI. Inline keys of that length are invalid YAML, so pnpm could not re-read the lockfile it had just written and every subsequent install re-resolved from scratch.
