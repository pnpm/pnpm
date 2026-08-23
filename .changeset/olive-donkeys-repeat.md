---
"@pnpm/fetching.binary-fetcher": patch
"pnpm": patch
---

Updated `adm-zip` to v0.6.0, which fixes [a memory-exhaustion vulnerability](https://github.com/advisories/GHSA-xcpc-8h2w-3j85) where a crafted ZIP file could make it allocate 4 GB of memory. `adm-zip` is used to extract the Node.js, Bun, and Deno archives that pnpm downloads on Windows.
