---
"pacquet": minor
"@pnpm/pnpr": minor
---

Removed platform variants. A package version has one archive again.

pnpm no longer reads `publishConfig.variants` or the `variant` field of `package.json`. `pnpm pack --variant` and the `--upload-variant`, `--variant-integrity` and `--no-variants` options of `pnpm publish` are gone. pnpr no longer accepts variant archives with a version or serves them at `<name>@<version>/variants/<tag>`.

To ship builds for several platforms, publish each as its own package and list them in `optionalDependencies`. A lockfile that records the variants of a registry package has to be resolved again.
