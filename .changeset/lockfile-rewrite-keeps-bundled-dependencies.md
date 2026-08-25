---
"pacquet": patch
---

Rewriting `pnpm-lock.yaml` no longer deletes `bundledDependencies` from package entries whose resolution was reused, so bumping one dependency no longer strips the field from every unrelated entry that carries it [#14153](https://github.com/pnpm/pnpm/issues/14153).
