---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Recover from a metadata cache entry that disappears (concurrent cache cleanup, antivirus) after the registry has already answered the conditional request with `304 Not Modified`. The metadata is re-requested once without cache validators instead of failing the install with `ERR_PNPM_CACHE_MISSING_AFTER_304`.
