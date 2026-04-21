---
"@pnpm/fetching.binary-fetcher": minor
"@pnpm/fetching.fetcher-base": minor
"@pnpm/fetching.tarball-fetcher": minor
"@pnpm/engine.runtime.node-resolver": major
"@pnpm/store.cafs": minor
"@pnpm/worker": minor
"pnpm": major
---

Installing a Node.js runtime via `node@runtime:<version>` (including `pnpm env use` and `pnpm runtime set node`) no longer extracts the bundled `npm`, `npx`, and `corepack` from the Node.js archive. This cuts roughly half of the files pnpm has to hash, write to the CAS, and link during installation, making runtime installs noticeably faster. Users who still need `npm` can install it as a separate package.
