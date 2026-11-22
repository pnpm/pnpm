---
"pacquet": patch
---

`pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Previously the flag was accepted but ignored.
