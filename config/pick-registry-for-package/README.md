# @pnpm/pick-registry-for-package

> Picks the right registry for the package from a registries config

## Installation

```
pnpm add @pnpm/pick-registry-for-package
```

## API

```ts
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'

pickRegistryForPackage(
  registries,         // { default: string, [scope: string]: string }
  packageName,        // e.g. '@foo/bar' or 'lodash' — can be a local alias
  bareSpecifier?,     // e.g. 'npm:@foo/bar@1.2.3' (for aliased deps)
  registryOverrides?, // { [exactPackageName: string]: string }
): string
```

Resolution order:

1. If `registryOverrides` contains an exact match for the real package name,
   that URL is returned. This is how the `registryOverrides` setting from
   `pnpm-workspace.yaml` lets a single package in a scope (e.g. `@foo/private`)
   be served from a different registry than the rest of the scope.
2. Otherwise, if the (resolved) package name is scoped and that scope is a key
   in `registries`, the scope's registry URL is returned.
3. Otherwise, `registries.default` is returned.

When `bareSpecifier` is an `npm:` aliased specifier (e.g. `npm:@foo/private@1`),
the real package name is extracted from the specifier for both the override
lookup and the scope lookup.

## Interactions with other registry-selection mechanisms

`pickRegistryForPackage` is not the only thing that can influence which
registry pnpm talks to for a given package. The full precedence, from highest
to lowest, is:

1. **Custom resolvers in `.pnpmfile.cjs`** — run before pnpm's built-in
   resolution and can bypass registries entirely (see `CustomResolver` in
   `@pnpm/hooks.types`).
2. **`publishConfig.registry` in `package.json`** — only consulted for publish
   operations; wins over `registryOverrides` during `pnpm publish`.
3. **`registryOverrides`** — introduced by this package; exact-name lookup.
4. **`registries[@scope]`** — npm-style scoped registry from `.npmrc` or
   `pnpm-workspace.yaml`.
5. **`registries.default`** — the default registry.

## Auth

The override does not introduce a new auth mechanism. Auth headers are looked
up by URL via `createGetAuthHeaderByURI` (`@pnpm/network.auth-header`), which
uses the same `nerfDart`-normalized `.npmrc` entries (`//host/:_authToken=...`,
`//host/:_auth=...`, `//host/:tokenHelper=...`, TLS certs, etc.). As long as
the override URL has credentials configured in `.npmrc`, no additional
configuration is required.

## License

[MIT](LICENSE)
