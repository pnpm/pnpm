---
"@pnpm/pnpr": minor
---

The package rules admin API now reports a `version` and an `ETag`, and accepts `If-Match` on `PUT` and `DELETE`. A change based on rules another admin has since changed is refused with `412`.
