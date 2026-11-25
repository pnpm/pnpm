---
"pnpm": patch
---

`pnpm patch` and `pnpm patch-commit` now support binary files. Files that contain non-text content are patched with git's binary patch format (`GIT binary patch`, literal and delta blocks) instead of producing mangled text diffs, and applying a patch writes binary files byte-for-byte. Re-applying an already-applied binary patch is a no-op, and deleted binary files are handled correctly.
