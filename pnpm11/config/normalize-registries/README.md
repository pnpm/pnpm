# @pnpm/config.normalize-registries

> Accepts a mapping of registry URLs and returns a mapping with the same URLs but normalized

## Installation

```
pnpm add @pnpm/config.normalize-registries
```

## Usage

```typescript
import { normalizeRegistriesByScope } from '@pnpm/config.normalize-registries'

normalizeRegistriesByScope({
  'default': 'https://registry.npmjs.org',
  '@foo': 'https://example.com',
})
// returns these registries merged over the defaults, with every URL ending in a /
```

## License

[MIT](LICENSE)
