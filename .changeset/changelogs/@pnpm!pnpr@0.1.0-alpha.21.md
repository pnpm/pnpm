## 0.1.0-alpha.21

### Minor Changes

- pnpr refuses a database created by an alpha release. Drop its pnpr tables together with the hosted store, and pnpr creates them afresh.

  On PostgreSQL, pnpr sorts and compares package names, versions, and other keys by bytes, as it does on SQLite and MySQL.

### Patch Changes

- pnpr now decides who may do what with grants. A grant gives accounts, teams or built-in groups a set of actions on the packages of one or more registries. The actions are `read`, `cache`, `publish`, `annotate`, `delete` and `manage`. Grants are kept in the database apart from the registries, so restoring an earlier revision of the registries does not change them.

  The `access` key of a registry and the `access`, `publish` and `unpublish` lists under its `packages` keys are removed. Adding a registry creates a default grant for it. The `/-/pnpr/v0/admin/rules` endpoints are replaced by `/-/pnpr/v0/admin/grants`, and the new `pnpr grants export` and `pnpr grants import` commands read and write the grants as a YAML file.

  Changing a dist-tag, a deprecation, a note or the maintainers now takes `annotate`. Reading through an upstream takes `read`, and a caller without `cache` gets only what the cache already holds. This changes the database schema and requires resetting existing alpha deployments.

- pnpr teams now belong to the instance, not to one registry. The access lists of every registry can name the same team as `team:<id>`. `pnpm team` edits one set of teams, whichever scope it is given. A signed-in caller who is not an admin sees the teams they are in.

  The `teams` key of a registry and the `registry` key of an OIDC group grant are removed. Removing an account removes it from every team. An `artifacts.owners` list can name a team. This changes the database schema and requires resetting existing alpha deployments.

- Fixed writes on PostgreSQL waiting until they timed out when more than one pnpr instance wrote through PgBouncer or another connection pool in transaction mode.

- A registry pattern that is not a valid package name now gets an error that names the patterns its ecosystem accepts. The error used to suggest `@scope/*` for every ecosystem, but only npm registries accept it.

- pnpr now serves a version published upstream after it cached the package's packument. A request for a version, its tarball, or its manifest that the cached packument does not list refetches the packument, at most once a minute per package. Before, pnpr answered `404` until `packumentTtl` expired [#18180](https://github.com/pnpm/pnpm/issues/18180).
