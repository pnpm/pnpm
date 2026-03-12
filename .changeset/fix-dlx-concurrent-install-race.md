---
"pnpm": patch
---

Fixed intermittent `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND` when multiple `pnpm dlx` calls run concurrently for the same package. When the install fails due to a concurrent global virtual store write, dlx now checks if another process has already completed the install and uses that cache instead of failing.
