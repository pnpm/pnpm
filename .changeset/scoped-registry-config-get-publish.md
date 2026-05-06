---
"@pnpm/config.commands": patch
"pnpm": patch
---

`pnpm config get @<scope>:registry` now reports the same URL that `pnpm publish` and the resolvers actually use. Previously, `config get` only consulted `.npmrc`, while `publish`/install used the merged map that includes `pnpm-workspace.yaml`'s `registries` block — so the two could diverge silently and a publish could go to the wrong registry [#11492](https://github.com/pnpm/pnpm/issues/11492).
