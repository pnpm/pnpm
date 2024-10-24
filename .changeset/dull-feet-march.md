---
"pnpm": major
---

Using SHA256 instead of md5 for hashing long peer dependency hashes in the lockfile. Should not affect a lot of users as the hashing is used for really long keys in the lockfile.
