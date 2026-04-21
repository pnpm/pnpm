---
"@pnpm/types": minor
"@pnpm/config.pick-registry-for-package": minor
"@pnpm/config.reader": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/deps.inspection.outdated": minor
"@pnpm/deps.inspection.commands": minor
"@pnpm/installing.env-installer": minor
"@pnpm/registry-access.commands": minor
"@pnpm/releasing.commands": minor
"@pnpm/store.connection-manager": minor
"pnpm": minor
---

Added a new `registryOverrides` setting for mixing public and private packages within the same scope. A package whose exact name matches a key in `registryOverrides` is resolved from the given registry URL, taking precedence over the scope's entry in `registries`. `pnpm publish` on an overridden package also targets the override URL. Authentication is picked up from the existing per-URL `.npmrc` entries (e.g. `//npm.pkg.github.com/:_authToken=...`), so no separate auth mechanism is required.

Example in `pnpm-workspace.yaml`:

```yaml
registryOverrides:
  "@foo/private-lib": https://npm.pkg.github.com/
  "@foo/internal-tools": https://npm.pkg.github.com/
```

With this, `@foo/public` still resolves from the default registry (or whatever `@foo:registry` is set to), while `@foo/private-lib` and `@foo/internal-tools` are fetched from GitHub Packages.
