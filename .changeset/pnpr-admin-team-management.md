---
"@pnpm/pnpr": minor
---

Admins can now manage teams with `pnpm team` on a hosted registry that sets `teamsManagedBy: api`. List the admins in `auth.admins`. pnpr stores these teams in the hosted store, so every replica serves the same teams, and a `team:` access rule applies a membership change immediately [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).

Browser origins listed in `cors.allowedOrigins` may now send `PUT`, `POST`, and `DELETE` requests.
