---
"pacquet": patch
---

Fixed `pnpm` crashing in StackBlitz WebContainers with `Parking not supported on this platform`. The crash was intermittent. It needed two threads to reach for the same internal lock at once.
