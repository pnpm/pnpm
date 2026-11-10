---
"@pnpm/pnpr": patch
---

pnpr database connection errors and retry logs now include the target and underlying cause, with credentials redacted. `pnpr config check` rejects insecure database token transport and database drivers missing from the build. Errors for a newer database schema now advise upgrading pnpr.
