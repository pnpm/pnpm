---
"@pnpm/pnpr": minor
---

A hosted registry serves the transparency log of its release records under `/-/log/`: the latest checkpoint, the hash tiles, and, for callers who may read every package of the organization, the entry bundles. A release record's evidence, `<name>/<version>+r<revision>/evidence`, holds the record's digest, the inclusion proof of its log entry against the log's checkpoint, and the version's provenance bundle. `GET /-/pnpr` lists `records` and `manifestProfiles` when the records are logged.
