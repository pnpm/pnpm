---
"@pnpm/pnpr": minor
---

pnpr accepts release archives uploaded by integrity before the publish that references them, through `PUT /-/pnpr/v0/artifacts/blob?integrity=<sha512-…>` without an owner [#17608](https://github.com/pnpm/pnpm/pull/17608).
