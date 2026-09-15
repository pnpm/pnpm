---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

A `registry` or `@scope:registry` set in an `.npmrc` now wins over the registry a `pnpm login` credential stored in the global `config.yaml` points at. Previously, after logging in to one registry, installs in a project whose `.npmrc` named a private registry went to the logged-in registry instead. They now go to the registry the `.npmrc` names [#14614](https://github.com/pnpm/pnpm/issues/14614).
