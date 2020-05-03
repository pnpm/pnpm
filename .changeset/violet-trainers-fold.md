---
"@pnpm/cafs": minor
"@pnpm/fetcher-base": minor
"@pnpm/git-fetcher": minor
"@pnpm/package-requester": minor
"pnpm": minor
"@pnpm/tarball-fetcher": minor
---

When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.
