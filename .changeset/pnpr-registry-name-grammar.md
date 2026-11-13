---
"@pnpm/pnpr": minor
---

A pnpr registry name must now start with a letter and contain only letters, digits, `.`, `_`, and `-`. It cannot be a reserved pnpm prefix name such as `npm`, `git`, `github`, or `workspace`. Rename a registry that breaks these rules before upgrading, and update the `/~<name>/` URLs that clients use. pnpr refuses to start on such a name in `pnpr.yaml`. With `registriesManagedBy: api`, pnpr starts but answers registry requests with `503` until an admin replaces the stored registries through the admin API.
