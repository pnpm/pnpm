---
"@pnpm/pnpr": minor
---

With `releaseRecords.statementSigningKey` set, a release record's evidence carries the registry's acceptance statement: an in-toto Statement in a DSSE envelope saying the registry accepted the record under its profile. pnpr signs it from the accepted record each time it is read, so rotating the key signs every statement again.
