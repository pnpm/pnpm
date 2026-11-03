---
"@pnpm/pnpr": minor
---

With `releaseRecords.statementSigningKey` set, a release record's evidence carries the registry's acceptance statement: a signed in-toto Statement saying the registry accepted the record under its profile. pnpr signs it from the accepted record each time it is read, so rotating the key signs every statement again.
