# @pnpm/normalize-registries

> Accepts a mapping of registry URLs and returns a mapping with the same URLs but normalized

## Installation

```
pnpm add @pnpm/normalize-registries
```

## Usage

```typescript
import normalizeRegistries from '@pnpm/normalize-registries'

normalizeRegistries({
  'default': 'https://registry.npmjs.org',
  '@foo': 'https://example.com',
})
// will return the same object but the URLs will end with a /
```

## License

[MIT](LICENSE)
