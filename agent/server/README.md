# pnpm-agent

A pnpm agent server that resolves dependencies server-side and streams only the files missing from the client's content-addressable store.

> **Status:** experimental. Versions are pre-1.0; the wire protocol may change between releases.

## How it works

1. Client sends `POST /v1/install` with dependencies, an optional existing lockfile, and the integrity hashes of packages already in its store.
2. Server resolves the full dependency tree using pnpm's own resolution engine.
3. Server computes which file digests the client is missing — at the individual file level, not just the package level.
4. Server streams an NDJSON response on `/v1/install` (`D`-lines for missing file digests, `I`-lines for pre-packed package index entries, a final `L`-line with the lockfile + stats, or an `E`-line on error).
5. The client then requests the missing file contents from `POST /v1/files`, which streams a gzip-compressed binary of packed file entries.

This eliminates sequential metadata round-trips (the server resolves in one shot) and avoids downloading files that already exist in the client's store from other packages.

## Starting the server

### Install from npm

```bash
pnpm add -g pnpm-agent
pnpm-agent
```

### Docker

A Dockerfile is provided at `agent/server/Dockerfile`. It is layered on top of [`ghcr.io/pnpm/pnpm`](https://github.com/pnpm/pnpm/pkgs/container/pnpm) and installs Node.js and `pnpm-agent` inside the image.

```bash
# Build the image locally
docker build -t pnpm-agent agent/server

# Run it, persisting the store + cache in ./agent-data
docker run --rm \
  -p 4873:4873 \
  -v "$(pwd)/agent-data:/agent-data" \
  pnpm-agent
```

Override the defaults with `-e`, same variables as described below:

```bash
docker run --rm \
  -p 4000:4000 \
  -e PORT=4000 \
  -e PNPM_AGENT_UPSTREAM=https://my-proxy.example.com/ \
  -v "$(pwd)/agent-data:/agent-data" \
  pnpm-agent
```

The image exposes port `4873` and declares a `/agent-data` volume; mount a host directory there if you want the resolved metadata, store index, and file store to survive container restarts.

### From source

```bash
# Build first
pnpm --filter pnpm-agent run compile

# Run with defaults (port 4873, upstream https://registry.npmjs.org/)
node lib/bin.js

# Or configure via environment variables
PORT=4000 \
PNPM_AGENT_STORE_DIR=./my-store \
PNPM_AGENT_CACHE_DIR=./my-cache \
PNPM_AGENT_UPSTREAM=https://registry.npmjs.org/ \
node lib/bin.js
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `4873` | Port to listen on |
| `PNPM_AGENT_STORE_DIR` | `./store` | Directory for the server's content-addressable store |
| `PNPM_AGENT_CACHE_DIR` | `./cache` | Directory for package metadata cache |
| `PNPM_AGENT_UPSTREAM` | `https://registry.npmjs.org/` | Upstream npm registry to resolve from |

### Programmatic usage

```typescript
import { createRegistryServer } from 'pnpm-agent'

const server = await createRegistryServer({
  storeDir: '/var/lib/pnpm-agent/store',
  cacheDir: '/var/lib/pnpm-agent/cache',
  registries: { default: 'https://registry.npmjs.org/' },
})

server.listen(4000, () => {
  console.log('pnpm agent listening on port 4000')
})
```

## Quick start

Terminal 1 — start the server:

```bash
cd agent/server
pnpm run compile
node lib/bin.js
# pnpm agent server listening on http://localhost:4873
```

Terminal 2 — use it from any project:

```bash
cd my-project
```

Add to `pnpm-workspace.yaml`:

```yaml
agent: http://localhost:4873
```

Or pass `--config.agent=http://localhost:4873` on the command line.

Then run:

```bash
pnpm install
```

That's it. pnpm will resolve dependencies on the server, download only the files missing from your local store, and link `node_modules` as usual. Remove the `agent` setting to go back to normal behavior.

## API

### `POST /v1/install`

**Request body** (JSON):

```json
{
  "projects": [
    {
      "dir": ".",
      "dependencies": { "react": "^19.0.0" },
      "devDependencies": { "typescript": "^5.0.0" }
    }
  ],
  "overrides": {},
  "lockfile": null,
  "storeIntegrities": ["sha512-abc...", "sha512-def..."]
}
```

**Response** (NDJSON, `Content-Type: application/x-ndjson`). Each line is one message:

- `D\t{digest}\t{size}\t{executable}` — file digest missing from the client's store.
- `I\t{integrity}\t{pkgId}\t{base64-msgpack}` — pre-packed package index entry.
- `L\t{json}` — final lockfile and stats. Emitted last on success.
- `E\t{json}` — error. Emitted if resolution fails.

### `POST /v1/files`

**Request body** (JSON):

```json
{ "digests": [{ "digest": "<hex>", "size": 123, "executable": false }] }
```

**Response** (gzip-compressed binary, `Content-Type: application/x-pnpm-install`):

```
[4 bytes: JSON metadata length]
[N bytes: JSON metadata]
[file entries: 64B digest + 4B size + 1B mode + content, repeated]
[64 zero bytes: end marker]
```
