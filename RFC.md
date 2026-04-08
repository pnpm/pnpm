# RFC: pnpm-registry — Server-Side Resolution and Store-Aware Downloads

## Summary

A pnpm-specific registry server that resolves dependencies server-side and streams only the files missing from the client's content-addressable store. With a warm server, install is ~33% faster than standard pnpm (15s vs 22s for a 1351-package project).

## How It Works

1. Client reads integrity hashes from its local store index
2. Sends `POST /v1/install` with dependencies + store integrities
3. Server resolves the full dependency tree using pnpm's own install pipeline
4. Server computes which file digests the client is missing (file-level dedup)
5. Server returns: lockfile + pre-packed msgpack store index entries + list of missing file digests
6. Client dispatches parallel `POST /v1/files` requests from worker threads
7. Each worker fetches a batch of files, decodes the binary response, and writes directly to CAFS (no rehashing, no temp files)
8. Client writes pre-packed store index entries to SQLite
9. Client runs headless install for linking

## Architecture

### Server (`@pnpm/registry.server`)

- **Multi-process**: Uses Node.js `cluster` module (default: CPU cores - 1 workers)
- **Resolution**: Calls pnpm's `install()` with `lockfileOnly: true` in a temp dir
- **Store**: Maintains its own CAFS with pre-extracted packages
- **Metadata cache**: Uses pnpm's standard npm metadata cache (`preferOffline` for fast repeat resolution)
- **File serving**: Reads files from CAFS, serves via `/v1/files` endpoint

### Client (`@pnpm/registry.client`)

- **Store integrities**: Reads package integrity hashes from local store index via `StoreIndex.keys()` (key-only SQL query, no msgpack decode)
- **File fetching**: Worker threads make HTTP requests directly to the server, decode the binary protocol, and write to CAFS using `writeFileSync` with `O_CREAT|O_EXCL` — no SHA-512 rehashing, no temp+rename
- **Store index**: Writes pre-packed msgpack buffers from the server directly to SQLite via `setRawMany`
- **Headless install**: Runs with the received lockfile for linking into `node_modules`

### Configuration

In `pnpm-workspace.yaml`:

```yaml
pnpmRegistry: http://localhost:4873
```

## Protocol

### `POST /v1/install`

**Request** (JSON):
```json
{
  "dependencies": { "react": "^19.0.0" },
  "devDependencies": { "typescript": "^5.0.0" },
  "overrides": {},
  "lockfile": null,
  "storeIntegrities": ["sha512-abc...", "sha512-def..."]
}
```

**Response** (binary):
```
[4 bytes: JSON length][JSON: lockfile + missingFiles + stats]
[package index entries: 2B key_len + key + 4B buf_len + msgpack]...
[2 zero bytes: end marker]
```

The JSON contains the resolved lockfile, a list of missing file digests with sizes, and stats. The binary section contains pre-packed msgpack store index entries that the client writes directly to SQLite.

### `POST /v1/files`

**Request** (JSON):
```json
{
  "digests": [
    { "digest": "abc123...", "size": 1234, "executable": false },
    ...
  ]
}
```

**Response** (binary):
```
[4 bytes: JSON length][JSON: {}]
[file entries: 64B digest + 4B size + 1B mode + content]...
[64 zero bytes: end marker]
```

Each file entry contains the raw file content. The client writes it directly to the CAFS path computed from the digest — no SHA-512 rehashing needed since the server already verified the content.

## Performance

Benchmarked with a 1351-package project (cold local store, warm server):

| Scenario | Time |
|----------|------|
| Standard pnpm install (no registry) | 22s |
| With pnpm-registry (cold server) | ~35s |
| With pnpm-registry (warm server, run 2) | ~15s |
| With pnpm-registry (warm server, run 3+) | **~15s** |

### Where Time Goes (warm server, ~15s)

| Phase | Time |
|-------|------|
| Server resolution (`lockfileOnly`) | ~3-5s |
| File download + worker CAFS writes | ~4-5s |
| Headless install (linking) | ~5-6s |

### Key Optimizations

1. **Multi-process server** (cluster): Parallel handling of `/v1/files` requests
2. **Worker-thread HTTP**: Client workers make HTTP requests and write to CAFS directly, bypassing the main thread
3. **No rehashing**: Files are written using the server-provided digest — skips 33K SHA-512 computations
4. **Direct write**: Uses `writeFileSync` with `O_CREAT|O_EXCL` instead of stat + temp + rename
5. **Pre-packed msgpack**: Server sends raw msgpack store index buffers, client writes them to SQLite without re-encoding
6. **Recent file skip**: `checkPkgFilesIntegrity` skips `stat()` for files checked within the last 60 seconds

## File-Level Dedup

The server computes which individual file digests the client is missing, not just which packages. Even for packages the client doesn't have, many files already exist in the store from other packages:

- `lodash@4.17.20` and `4.17.21` share 595/600 files
- Many packages share identical `LICENSE` files
- Version upgrades download only the changed files

For the 1351-package test project: 39,330 total file references, 33,092 unique digests (6,238 deduped across packages).

## Usage

### Starting the Server

```bash
cd registry/server
pnpm run compile
node lib/bin.js
```

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `4873` | Port to listen on |
| `PNPM_REGISTRY_STORE_DIR` | `./store` | Server's content-addressable store |
| `PNPM_REGISTRY_CACHE_DIR` | `./cache` | Package metadata cache |
| `PNPM_REGISTRY_UPSTREAM` | `https://registry.npmjs.org/` | Upstream npm registry |
| `PNPM_REGISTRY_WORKERS` | `CPU cores - 1` | Number of cluster workers |

### Using from a Project

Add to `pnpm-workspace.yaml`:

```yaml
pnpmRegistry: http://localhost:4873
```

Then `pnpm install` uses the registry server automatically. Remove the setting to go back to normal behavior.
