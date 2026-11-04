---
"@pnpm/pnpr": minor
---

An npm publish to pnpr, alone or in a batch, can reference an archive staged before it: the attachment carries the archive's `integrity` in place of `data`. A version and its platform variants therefore no longer have to fit in one request body. An archive the publishing account has not staged, or one that has expired, fails the whole publish.
