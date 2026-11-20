---
"@pnpm/pnpr": patch
---

pnpr database connection diagnostics now include the target and underlying cause, with credentials redacted. `pnpr config check` rejects insecure database token transport. It also rejects database drivers missing from the build. Errors for a newer database schema now advise upgrading pnpr.
