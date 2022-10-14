# @pnpm/assert-store

> Utils for testing pnpm stores

## Installation

```
pnpm install -D @pnpm/assert-store
```

## Usage

```ts
import test = require('tape')
import { assertStore } from '@pnpm/assert-store'

test('...', async t => {
  // ...
  const store = assertStore(t, pathToStore, encodedRegistryName)

  await store.storeHas('is-positive', '3.1.0')
  // Test fails if pnpm store does not have this package
})
```

## License

MIT
