---
"@pnpm/resolving.named-registry-specifier-parser": major
"@pnpm/config.normalize-registries": major
"@pnpm/config.reader": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.default-resolver": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/store.connection-manager": minor
"@pnpm/types": minor
"pnpm": minor
---

Added support for installing packages from the [GitHub Packages npm registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry) via a built-in `gh:` prefix (e.g. `pnpm add gh:@acme/private`), and, more broadly, for arbitrary named registries in the style of [vlt's named-registry aliases](https://docs.vlt.sh/cli/registries). Authentication is picked up from the existing per-URL `.npmrc` entries (e.g. `//npm.pkg.github.com/:_authToken=...`), so no separate auth mechanism is required.

Additional aliases — or an override for the built-in `gh` alias, for GitHub Enterprise Server — can be configured under `namedRegistries` in `pnpm-workspace.yaml`:

```yaml
namedRegistries:
  gh: https://npm.pkg.github.example.com/
  work: https://npm.work.example.com/
```

With this, `work:@corp/lib@^2.0.0` resolves against `https://npm.work.example.com/`. [#8941](https://github.com/pnpm/pnpm/issues/8941).
