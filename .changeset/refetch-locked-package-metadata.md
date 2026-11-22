---
"pnpm": patch
---

`pnpm install --frozen-lockfile` now revalidates registry metadata when verifying a locked tarball whose version is missing from the cached package listing.
