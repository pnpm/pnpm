---
"@pnpm/pnpr": patch
---

pnpr now mints personal bearer tokens with the `pnpr_` prefix, a base62 payload, and an IEEE CRC-32 checksum. `POST /-/npm/v1/tokens` accepts only `"classic"` or `"granular"` for `kind`, and token responses omit `scopes`. pnpr deletes expired bearer tokens from storage at startup and every hour.

Account records in SQL databases and `auth.users.file` now store `status`, `credential_kind`, nullable `password_hash`, and `created_at` fields, and `tokens` omits `scopes`. Start upgraded alpha servers with fresh auth state [zkochan/pnpm-tasks#81](https://github.com/zkochan/pnpm-tasks/issues/81).
