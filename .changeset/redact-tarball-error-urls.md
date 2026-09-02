---
"pacquet": patch
---

Tarball download errors no longer print the inline `user:pass@` credentials of the URL they name, so a failed install or `pnpm add <url>` cannot leak them into terminal scrollback or CI logs.
