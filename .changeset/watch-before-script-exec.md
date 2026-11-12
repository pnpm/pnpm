---
"pacquet": patch
---

Scripts now stop when pnpm's process group is killed while they are starting. A script could previously start before its process group was registered for cleanup.
