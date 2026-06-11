---
"@pnpm/store.create-cafs-store": patch
"pnpm": patch
---

Create shorter CAFS temporary package directories to leave room for lifecycle scripts that create IPC socket paths under TMPDIR.
