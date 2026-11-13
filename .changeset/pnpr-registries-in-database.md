---
"@pnpm/pnpr": minor
---

pnpr can now keep its registries in the metadata database. Set `registriesManagedBy: api`, and admins listed in the new `auth.registryAdmins` can add, change, and remove registries through `/-/pnpr/v0/admin/registries` without a config deploy. Every replica serves the same registries, and each change is a revision that can be restored.

`pnpr registries import` and `pnpr registries export` move registries between a YAML file and the database. A stored upstream credential names an environment variable that the config lists in `registryEnv`.

`registriesManagedBy: api` replaces `teamsManagedBy: api` and `rulesManagedBy: api`, which pnpr now rejects. The team endpoints, `npm access grant`, and the rules endpoints edit the stored registries. Teams and rule changes stored under the old settings are not carried over.
