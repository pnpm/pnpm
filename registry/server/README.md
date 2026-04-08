# @pnpm/registry.server

A pnpm registry server that resolves dependencies server-side and streams only the files missing from the client's content-addressable store.

## How it works

1. Client sends `POST /v1/install` with dependencies, an optional existing lockfile, and the integrity hashes of packages already in its store.
2. Server resolves the full dependency tree using pnpm's own resolution engine.
3. Server computes which file digests the client is missing — at the individual file level, not just the package level.
4. Server streams a binary response: JSON metadata (lockfile + per-package file indexes) followed by the raw content of missing files.

This eliminates sequential metadata round-trips (the server resolves in one shot) and avoids downloading files that already exist in the client's store from other packages.

## Starting the server

### From the command line

```bash
# Build first
pnpm --filter @pnpm/registry.server run compile

# Run with defaults (port 4873, upstream https://registry.npmjs.org/)
node lib/bin.js

# Or configure via environment variables
PORT=4000 \
PNPM_REGISTRY_STORE_DIR=./my-store \
PNPM_REGISTRY_CACHE_DIR=./my-cache \
PNPM_REGISTRY_UPSTREAM=https://registry.npmjs.org/ \
node lib/bin.js
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `4873` | Port to listen on |
| `PNPM_REGISTRY_STORE_DIR` | `./store` | Directory for the server's content-addressable store |
| `PNPM_REGISTRY_CACHE_DIR` | `./cache` | Directory for package metadata cache |
| `PNPM_REGISTRY_UPSTREAM` | `https://registry.npmjs.org/` | Upstream npm registry to resolve from |

### Programmatic usage

```typescript
import { createRegistryServer } from '@pnpm/registry.server'

const server = await createRegistryServer({
  storeDir: '/var/lib/pnpm-registry/store',
  cacheDir: '/var/lib/pnpm-registry/cache',
  registries: { default: 'https://registry.npmjs.org/' },
})

server.listen(4000, () => {
  console.log('pnpm-registry listening on port 4000')
})
```

## Configuring pnpm to use the server

Add to `.npmrc`:

```ini
pnpm-registry=http://localhost:4000
```

Then `pnpm install` will use the registry server for resolution and fetching instead of the normal flow.

## API

### `POST /v1/install`

**Request body** (JSON):

```json
{
  "dependencies": { "react": "^19.0.0" },
  "devDependencies": { "typescript": "^5.0.0" },
  "overrides": {},
  "lockfile": null,
  "storeIntegrities": ["sha512-abc...", "sha512-def..."]
}
```

**Response** (binary, `Content-Type: application/x-pnpm-install`):

```
[4 bytes: JSON metadata length]
[N bytes: JSON metadata — lockfile, package file indexes, stats]
[file entries: 64B digest + 4B size + 1B mode + content, repeated]
[64 zero bytes: end marker]
```
