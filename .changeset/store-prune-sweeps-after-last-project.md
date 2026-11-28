---
"pnpm": patch
---

`pnpm store prune` now removes global virtual store packages after every project that used them is deleted. An install that uses the global virtual store now fails if it cannot register the project in the store.
