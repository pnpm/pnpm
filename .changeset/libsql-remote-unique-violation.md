---
"@pnpm/pnpr": patch
---

pnpr now starts against a Turso auth database it has already set up. The second start failed with a unique-constraint error on the user counter. Registering a username that is already taken on Turso now gets the same answer as on a local database.
