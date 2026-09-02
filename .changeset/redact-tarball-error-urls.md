---
"pacquet": patch
---

Tarball download and resolution errors no longer print the secrets of the URL they name — inline `user:pass@` credentials, and the query string or fragment of a signed URL — so a failed install or `pnpm add <url>` cannot leak them into terminal scrollback or CI logs.
