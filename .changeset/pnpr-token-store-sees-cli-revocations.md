---
"@pnpm/pnpr": patch
---

Changing a password with `pnpr add-user` while the registry runs now revokes the account's existing tokens right away. Before, the running registry kept accepting them until it restarted.
