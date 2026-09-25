# @pnpm/catalogs.resolver

> Dereferences `catalog:` protocol specifiers into usable specifiers.

## Usage

### `resolveFromCatalog`

Dereferences a wanted dependency using the catalog protocol and returns the configured specifier.

Returns one of three resolution result types:
- `found`: The catalog entry was found and resolved to a usable specifier.
- `unused`: The dependency specifier does not use the `catalog:` protocol.
- `misconfiguration`: The catalog entry is missing, uses recursive catalog definitions, or uses unsupported protocols (`link:`, `file:`).

```ts
import { resolveFromCatalog } from '@pnpm/catalogs.resolver'

const result = resolveFromCatalog(catalogs, {
  alias: 'react',
  bareSpecifier: 'catalog:',
})

if (result.type === 'found') {
  console.log(result.resolution.specifier) // e.g. '^19.0.0'
}
```

### `matchCatalogResolveResult`

Utility pattern matcher for handling the three resolution result variants.

```ts
import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'

const result = resolveFromCatalog(catalogs, wantedDependency)

matchCatalogResolveResult(result, {
  found: ({ resolution }) => console.log('Resolved:', resolution.specifier),
  unused: () => console.log('Not using catalog protocol'),
  misconfiguration: ({ error }) => {
    throw error
  },
})
```
