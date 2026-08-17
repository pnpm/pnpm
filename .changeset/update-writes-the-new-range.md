---
"pacquet": patch
---

`pnpm update` now writes the new version range back to `package.json` (and to the `catalog:` entry a dependency points at), instead of only updating the lockfile [#13879](https://github.com/pnpm/pnpm/issues/13879). The range operator the dependency already declared is preserved, and a dependency declared through a dist-tag (`"foo": "latest"`) keeps tracking the tag under both `pnpm update` and `pnpm update --latest`.
