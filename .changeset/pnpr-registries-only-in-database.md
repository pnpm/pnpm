---
"@pnpm/pnpr": minor
---

pnpr now keeps its registries in the metadata database instead of the config file. Every replica serves the same registries, and admins listed in the new `auth.registryAdmins` change them through `/-/pnpr/v0/admin/registries` without a config deploy. Each change is a revision that can be restored.

A config file that declares `registries`, `defaultRegistry`, or `routes.public` is refused at startup. Move those blocks into a file of their own and store it with `pnpr registries import`. `pnpr registries export` prints the stored registries in that format. `pnpr registries import --check` fails when the stored registries differ from a file.

On its first start against an empty database, pnpr stores a default set of registries: a proxy of the public npm registry and a hosted registry for private packages.

An upstream credential must be a `${NAME}` reference to an environment variable that the config lists in `registryEnv`. An upstream URL with a user name, password, or query string is refused.

`teamsManagedBy` and `rulesManagedBy` are removed. The team endpoints, `npm access grant`, and the rules endpoints now edit the stored registries. Teams and rule changes stored under the old settings are not carried over.
