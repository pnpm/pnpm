---
"@pnpm/pnpr": minor
---

`npm access grant`, `npm access revoke`, and `npm access list packages <scope:team>` now work on a registry with `rulesManagedBy: api`. A grant adds the team to the `access` list of a package the `packages:` map declares by name, and `read-write` also adds it to `publish`.
