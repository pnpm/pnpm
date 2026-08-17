---
"pacquet": patch
---

`pnpm outdated` and `pnpm update --interactive` now dereference `catalog:` specifiers before querying the registry. A catalog entry that is an npm alias (`'@types/zkochan__table': npm:@types/table@6.3.2`) no longer fails with `ERR_PNPM_OUTDATED_REGISTRY_ERROR` for the alias key, and `outdated --compatible` compares against the range the catalog holds instead of skipping the dependency. When a packument request does fail, the reported reason is now the registry's HTTP status (`404 Not Found`) rather than "error decoding response body".
