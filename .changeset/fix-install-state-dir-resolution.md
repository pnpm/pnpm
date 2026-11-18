---
"pnpm": patch
---

Fixed an issue where `pnpm clean` failed to properly resolve and clean the `installStateDir` when using an external `virtualStoreDir`.
