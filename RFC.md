# RFC: pnpm-registry — Server-Side Resolution and Store-Aware Downloads

## Summary

A pnpm-specific registry server that resolves dependencies server-side and streams only the files missing from the client's content-addressable store. With a warm server, install is faster than standard pnpm (~10s vs ~11-12s for a 1351-package project, despite pnpm benefiting from warm npm CDN edge caches).

## How It Works

1. Client reads integrity hashes from its local store index
2. Sends `POST /v1/install` with dependencies + store integrities
3. Server resolves the full dependency tree using pnpm's own install pipeline, with a SQLite-backed metadata cache for fast repeat resolution
4. As each package resolves, the server immediately streams the digests of files the client is missing (file-level dedup)
5. After resolution, the server sends the final lockfile + pre-packed msgpack store index entries
6. Client reads the streaming response line by line; as digest batches fill up, it dispatches worker threads to `POST /v1/files` — file downloads overlap with resolution
7. Each worker fetches a batch of files, decodes the binary response, and writes directly to CAFS (no rehashing, no temp files)
8. Client writes pre-packed store index entries to SQLite
9. Client runs headless install for linking, with a wrapped store controller that awaits file downloads before each package is imported

## Architecture

### Server (`@pnpm/registry.server`)

- **Multi-process**: Uses Node.js `cluster` module (default: CPU cores - 1 workers)
- **Resolution**: Calls pnpm's `install()` with `lockfileOnly: true` in a temp dir
- **Store**: Maintains its own CAFS with pre-extracted packages
- **Metadata cache (SQLite)**: A `MetadataStore` class stores package metadata as blobs in SQLite, keyed by cache key. On startup, the existing `.jsonl` metadata cache files are imported into SQLite. The resolver reads metadata via a SQLite-backed `PackageMetaCache` implementation — one indexed DB lookup per package instead of reading/parsing multi-MB JSON files from disk. Plain pnpm CLI is unaffected; it continues using the default LRU + `.jsonl` cache.
- **File store (SQLite)**: A `FileStore` class stores CAFS file contents as blobs in SQLite. The `/v1/files` endpoint reads from SQLite instead of thousands of individual `readFileSync` calls. Files are lazily cached: on first request they're read from CAFS and stored in SQLite; subsequent requests serve directly from SQLite. The import runs in the background via `setImmediate` so it doesn't block the `/v1/install` response.
- **Streaming `/v1/install`**: The response is NDJSON. A wrapped `storeController.requestPackage` intercepts each resolved package, looks up its files in the integrity index, and emits digest lines immediately to the response stream. The final lockfile and pre-packed msgpack index entries are sent after resolution completes.

### Client (`@pnpm/registry.client`)

- **Store integrities**: Reads package integrity hashes from local store index via `StoreIndex.keys()` (key-only SQL query, no msgpack decode)
- **Streaming response parser**: Reads the NDJSON response line by line. As digest lines accumulate into batches of 4000, dispatches worker threads to `/v1/files`. File downloads begin while the server is still resolving.
- **Worker-thread file fetching**: Workers make HTTP requests directly to the server, decode the binary file protocol, and write to CAFS using `writeFileSync` with `O_CREAT|O_EXCL` — no SHA-512 rehashing, no temp+rename
- **Store index**: Writes pre-packed msgpack buffers from the server directly to SQLite via `setRawMany`
- **Headless install**: Runs with the received lockfile for linking into `node_modules`, with a wrapped store controller that awaits file downloads on first package access

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

**Response** (NDJSON, streamed):

```
D\t<digest>\t<size>\t<executable>\n
D\t<digest>\t<size>\t<executable>\n
...
L\t{"lockfile":{...},"stats":{...}}\n
I\t<key>\t<base64-msgpack>\n
I\t<key>\t<base64-msgpack>\n
...
```

Each line is a single typed message:
- `D` — **digest**: a file the client is missing. Sent as packages resolve on the server.
- `L` — **lockfile**: the final resolved lockfile and stats. Sent after resolution completes.
- `I` — **index entry**: a pre-packed msgpack store index buffer that the client writes directly to SQLite.

The client parses lines as they arrive, batches digest lines, and dispatches workers to `/v1/files` while the server is still sending more digests.

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
| Standard pnpm install (cold npm CDN edge cache) | ~18s |
| Standard pnpm install (warm npm CDN edge cache) | ~11-12s |
| With pnpm-registry (first run, cold SQLite caches) | ~12-14s |
| With pnpm-registry (warm SQLite caches) | **~10s** |

### Where Time Goes (warm server, ~10s)

| Phase | Time |
|-------|------|
| Server resolution (`lockfileOnly` with SQLite metadata) | ~1s |
| File download (overlapped with resolution) | ~3s |
| Headless install (linking) | ~5-6s |

### Key Optimizations

1. **SQLite metadata cache**: Server uses a SQLite-backed `PackageMetaCache` instead of reading `.jsonl` files from disk. Resolution time drops from ~3.4s to ~0.9s on warm runs.
2. **SQLite file store**: `/v1/files` reads from one SQLite file instead of 33K individual `readFileSync` calls.
3. **Streaming `/v1/install`**: File digests are streamed as packages resolve. Workers start downloading files while the server is still resolving.
4. **Multi-process server** (cluster): Parallel handling of `/v1/files` requests
5. **Worker-thread HTTP**: Client workers make HTTP requests and write to CAFS directly, bypassing the main thread
6. **No rehashing**: Files are written using the server-provided digest — skips 33K SHA-512 computations
7. **Direct write**: Uses `writeFileSync` with `O_CREAT|O_EXCL` instead of stat + temp + rename
8. **Pre-packed msgpack**: Server sends raw msgpack store index buffers, client writes them to SQLite without re-encoding
9. **Recent file skip**: `checkPkgFilesIntegrity` skips `stat()` for files checked within the last 60 seconds
10. **Pipelined headless install**: Wrapped store controller awaits file downloads per-package, so headless install starts before all files are downloaded

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
