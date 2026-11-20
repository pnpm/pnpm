---
"@pnpm/pnpr": minor
---

With SQLite or libSQL metadata, a hosted npm version is accepted with its release record, and `<name>/<version>+r0` serves the record's bytes as they were accepted. The record starts with the fields every release record has: `schema`, `purl`, `revision`, `interpretation`, the profile its manifest was derived under, and `inputs`, the digests of the `.tgz` and the `package.json` it was derived from. Each platform variant lists its own `inputs`. The metadata database's schema changed, so a database written by an earlier pnpr is refused.
