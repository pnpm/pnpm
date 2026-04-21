---
"@pnpm/fetching.binary-fetcher": patch
"pnpm": patch
---

Fix Windows Node.js runtime installs still extracting bundled `npm`, `npx`, and `corepack` when the archive contains explicit directory entries. `extractZipToTarget` now skips directory entries: AdmZip's `extractEntryTo` for a directory recurses over every descendant internally, which bypassed the `ignoreEntry` filter and re-materialized the files the filter was supposed to drop. File extraction creates parent directories implicitly, so skipping directory entries doesn't regress the install layout.
