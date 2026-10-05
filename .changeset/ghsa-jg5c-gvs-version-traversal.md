---
"pacquet": patch
---

`pnpm install` now validates dependency versions and slot directories in the global virtual store, rejecting path traversal attempts before creating slot directories.
