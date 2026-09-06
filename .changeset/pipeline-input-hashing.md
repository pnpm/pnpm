---
"pacquet": patch
---

`pnpm pipeline` now reports unreadable inputs and non-UTF-8 filenames when computing task cache keys. These inputs were silently omitted, which could reuse stale results. Unix filenames containing backslashes are now hashed as literal paths.
