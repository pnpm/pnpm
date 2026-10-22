---
"@pnpm/pnpr": minor
---

Admins listed in `auth.admins` can now manage accounts through `/-/pnpr/v0/admin/users`. They can create accounts while self-registration is disabled, change passwords, remove accounts, and revoke tokens. Removing an account also revokes its tokens [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).
