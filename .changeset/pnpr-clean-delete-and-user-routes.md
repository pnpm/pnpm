---
"@pnpm/pnpr": minor
---

Added `DELETE /{name}`, `DELETE /@{scope}/{name}`, `DELETE /{name}@{version}`, `DELETE /@{scope}/{name}@{version}`, `DELETE /{name}/-/{filename}`, and `DELETE /@{scope}/{name}/-/{filename}` routes that do not require a trailing `/-rev/{rev}` segment. `PUT /-/user/{name}` and `GET /-/user/{name}` now accept bare usernames in addition to the legacy `org.couchdb.user:` prefix.
