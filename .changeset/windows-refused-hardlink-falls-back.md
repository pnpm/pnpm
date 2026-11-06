---
"pacquet": patch
---

On Windows, with `packageImportMethod: auto`, an install no longer fails when the store refuses a hardlink with an access-denied error. pnpm tries cloning or copying the file next.
