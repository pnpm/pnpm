---
"@pnpm/pnpr": minor
---

OIDC sign-in can now put a user in registry teams based on the groups in their ID token. Configure it under `auth.oidc[].login.groups`. The membership lasts as long as the session and does not apply to the user's password logins or tokens [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).
