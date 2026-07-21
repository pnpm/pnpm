---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

Fixed `pnpm add --virtual-store-dir` to use the requested virtual store directory consistently, preserve the configured or default directory for empty values, and report global-install conflicts before loading configuration.
