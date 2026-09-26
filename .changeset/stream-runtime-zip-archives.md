---
"@pnpm/fetching.binary-fetcher": patch
"pnpm": patch
---

Installing a runtime from a zip archive, such as Node.js on Windows, Deno, or Bun, uses less memory. pnpm now writes the download to disk as it arrives and extracts one entry at a time [#14164](https://github.com/pnpm/pnpm/issues/14164).
