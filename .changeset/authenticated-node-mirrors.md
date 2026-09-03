---
"@pnpm/crypto.shasums-file": patch
"@pnpm/engine.runtime.commands": patch
"@pnpm/engine.runtime.node-resolver": minor
"@pnpm/fetching.binary-fetcher": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.client": patch
"@pnpm/network.fetch": patch
"@pnpm/resolving.default-resolver": patch
"pacquet": patch
"pnpm": patch
---

Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).
