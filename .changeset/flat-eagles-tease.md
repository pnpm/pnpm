---
"pnpm": minor
---

When patching a dependency that is already patched, the existing patch is applied to the dependency, so that the new edit are applied on top of the existing ones. To ignore the existing patches, run the patch command with the `--ignore-existing` option [#5632](https://github.com/pnpm/pnpm/issues/5632).
