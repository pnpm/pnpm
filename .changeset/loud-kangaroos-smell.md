---
"@pnpm/git-fetcher": patch
"@pnpm/tarball-fetcher": patch
---

Do not remove the Git temporary directory because it might still be in the process of linking to the CAFS.
