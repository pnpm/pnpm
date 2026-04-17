# @pnpm/resolving.registry-metadata-cache

> SQLite-backed binary cache for npm registry metadata

## Installation

```sh
pnpm add @pnpm/resolving.registry-metadata-cache
```

## Usage

```typescript
import { RegistryMetadataCache } from '@pnpm/resolving.registry-metadata-cache'
import type { PackageMeta } from '@pnpm/resolving.registry-types'

const cache = new RegistryMetadataCache('/path/to/cache')

// Store metadata
cache.set('lodash', 'https://registry.npmjs.org', packageMeta)

// Retrieve metadata
const meta: PackageMeta | undefined = cache.get('lodash', 'https://registry.npmjs.org')

// Check if metadata exists
const exists: boolean = cache.has('lodash', 'https://registry.npmjs.org')

// Get HTTP cache headers
const headers = cache.getHeaders('lodash', 'https://registry.npmjs.org')
// Returns: { etag?: string, modified?: string }

// Cleanup
cache.close()
```

## Features

- **SQLite-backed storage** with WAL mode for concurrent access
- **MessagePack serialization** for fast binary encoding/decoding
- **HTTP cache header tracking** (ETag and Last-Modified) for conditional requests
- **Thread-safe** with retry logic for busy situations
- **Low memory footprint** with streaming operations

## Why Binary Cache?

When resolving dependencies, pnpm fetches package metadata from npm registries as JSON. JSON parsing can be a significant overhead, especially for packages with many versions. This binary cache stores the parsed metadata using MessagePack, which:

- Eliminates redundant JSON parsing overhead
- Provides faster serialization/deserialization
- Maintains full fidelity of the metadata structure
- Reduces CPU usage during repeated resolution operations

## License

MIT