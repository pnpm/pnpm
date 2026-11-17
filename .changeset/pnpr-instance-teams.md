---
"@pnpm/pnpr": patch
---

pnpr teams now belong to the instance, not to one registry. The access lists of every registry can name the same team as `team:<id>`, and `pnpm team` edits one set of teams whichever scope it is given. A signed-in caller who is not an admin sees the teams they are in.

The `teams` key of a registry and the `registry` key of an OIDC group grant are removed. Removing an account removes it from every team. An `artifacts.owners` list can name a team. This changes the database schema and requires resetting existing alpha deployments.
