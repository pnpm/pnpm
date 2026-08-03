---
"@pnpm/store.path": patch
"pnpm": patch
---

Fix `dirsAreEqual` to correctly check for empty relative path, ensuring that the store is placed in the home directory when the project root itself is a mountpoint and the parent directory is not linkable pnpm/pnpm#13602.
