---
"pacquet": patch
---

Fixed a rare hang where `pnpm install` or `pnpm add` could wait forever: when two tasks fetched the same tarball concurrently, the waiting task could miss the downloader's completion notification and never wake up.
