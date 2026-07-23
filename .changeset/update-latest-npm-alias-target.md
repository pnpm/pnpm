---
"pacquet": patch
---

`pnpm update --latest` now resolves a dependency declared through an `npm:` alias — directly in `package.json` or in the catalog entry a `catalog:` reference points to — to the latest version of the aliased package, keeping the `npm:<name>@` prefix in the rewritten specifier. Previously the alias name itself was looked up on the registry, failing the update with a 404 when no package of that name exists.
