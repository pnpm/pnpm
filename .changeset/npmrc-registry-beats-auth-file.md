---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

A `registry` or `@scope:registry` set in an `.npmrc` now keeps its place when the global `config.yaml` holds an `_auth` credential for another registry. Previously, after a `pnpm login` to one registry, installs in a project whose `.npmrc` pointed at a private registry went to the logged-in registry instead. The credential's inferred route now only fills in a registry that no config file declares [#14614](https://github.com/pnpm/pnpm/issues/14614).
