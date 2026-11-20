---
"@pnpm/pnpr": patch
---

`pnpm search` and `npm search` against an upstream with `search: true` no longer fail with a 400 error on broad terms. pnpr now truncates an upstream that holds more results than its fetch budget downloads. The results it could not download keep `total` approximate.
