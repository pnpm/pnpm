---
"pnpm": patch
---

After a workspace script runs, pnpm links commands from scoped injected packages into the containing modules directory's `.bin`. Commands declared through `directories.bin` are also refreshed.
