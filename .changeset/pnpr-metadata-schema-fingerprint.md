---
"@pnpm/pnpr": patch
---

pnpr marks its SQL metadata database with a fingerprint of its schema instead of a version number. A database that holds another schema is refused before pnpr creates anything in it, with an error that names both fingerprints and says to wipe pnpr's metadata tables, since pnpr does not migrate them before 1.0.
