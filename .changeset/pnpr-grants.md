---
"@pnpm/pnpr": patch
---

pnpr now decides who may do what with grants. A grant gives accounts, teams or built-in groups a set of actions on the packages of one or more registries. The actions are `read`, `cache`, `publish`, `annotate`, `delete` and `manage`. Grants are kept in the database apart from the registries, so restoring an earlier revision of the registries does not change them.

The `access` key of a registry and the `access`, `publish` and `unpublish` lists under its `packages` keys are removed. Adding a registry creates a default grant for it. The `/-/pnpr/v0/admin/rules` endpoints are replaced by `/-/pnpr/v0/admin/grants`, and the new `pnpr grants export` and `pnpr grants import` commands read and write the grants as a YAML file.

Changing a dist-tag, a deprecation, a note or the maintainers now takes `annotate`. Reading through an upstream takes `read`, and a caller without `cache` gets only what the cache already holds. This changes the database schema and requires resetting existing alpha deployments.
