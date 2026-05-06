---
"@pnpm/config.commands": patch
"@pnpm/config.reader": patch
"pnpm": patch
---

`pnpm config get @<scope>:registry` now reports the same URL that `pnpm publish` and the resolvers actually use. Previously, `config get` only consulted `.npmrc`, while `publish`/install used the merged map that includes `pnpm-workspace.yaml`'s `registries` block — so the two could diverge silently and the user would publish to the wrong registry. pnpm now also emits a warning at config load time when the same scope (or the default registry) is defined in both `.npmrc` and `pnpm-workspace.yaml` with different values [#11492](https://github.com/pnpm/pnpm/issues/11492).
