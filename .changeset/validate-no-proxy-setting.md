---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

Reject unsupported types for the `noProxy` setting in workspace and global configuration. Strings and booleans remain valid, and `null` leaves the setting unset. Previously, unsupported values could reach proxy resolution without a validation error.
