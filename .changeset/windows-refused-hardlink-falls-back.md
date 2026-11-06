---
"pacquet": patch
---

On Windows, an install no longer fails when the store refuses a hardlink with an access-denied error. pnpm falls back to cloning or copying the file.
