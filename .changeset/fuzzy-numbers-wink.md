---
"@pnpm/cafs": major
"@pnpm/fetcher-base": major
---

`generatingIntegrity` replaced with `writeResult`. When files are added to the store, the store returns not only the file's integrity as a result, but also the exact time when the file's content was verified with its integrity.
