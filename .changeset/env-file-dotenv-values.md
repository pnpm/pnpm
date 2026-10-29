---
"pacquet": patch
---

`pnpm --env-file` now reads files the way `node --env-file` does. Values are taken as written, so `$VAR` is no longer expanded and Windows paths such as `C:\Users\me` load unchanged. A file is no longer rejected for its contents.
