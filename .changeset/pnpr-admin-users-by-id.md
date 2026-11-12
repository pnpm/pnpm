---
"@pnpm/pnpr": minor
---

Added `POST /-/pnpr/v0/admin/users` to create accounts through the admin API. `PUT /-/pnpr/v0/admin/users/{id}` no longer creates accounts. It replaces an existing account's password by account ID and returns `404` for unknown IDs [zkochan/pnpm-tasks#82](https://github.com/zkochan/pnpm-tasks/issues/82).
