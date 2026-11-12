---
"pacquet": patch
---

If the pnpr server refuses a setting the install uses, or cannot return the lockfile format pnpm reads, `pnpm install` and `pnpm update` print a warning and resolve locally. Cargo and Python resolution also fall back to resolving locally. pnpm now rejects a pnpr resolution of npm dependencies that does not name its lockfile format.
