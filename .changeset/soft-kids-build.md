---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Fixed a race condition in temporary file creation in the store by including worker thread ID in filename. Previously, multiple worker threads could attempt to use the same temporary file. Temporary files now include both process ID and thread ID for uniqueness [#8703](https://github.com/pnpm/pnpm/pull/8703).
