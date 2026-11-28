---
"pnpm": patch
---

When pnpm resolves a runtime from its `SHASUMS256.txt` file, it now skips a file whose hash is not 64 hex digits. It used to write an empty or wrong integrity for that file to the lockfile.
