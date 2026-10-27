---
"@pnpm/fetching.binary-fetcher": patch
"@pnpm/installing.client": patch
"pnpm": patch
---

`pnpm install` now retries a Node.js runtime zip download when the connection drops partway through. A download that still fails names its URL. Before, a single dropped connection ended the install with `terminated` [#17411](https://github.com/pnpm/pnpm/issues/17411).
