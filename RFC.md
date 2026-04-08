# RFC: pnpm-registry — Server-Side Resolution and Store-Aware Downloads

## Summary

A pnpm-specific registry layer that runs dependency resolution server-side and uses knowledge of the client's content-addressable store to deliver only the exact files that are missing. This reduces a typical install from dozens of sequential HTTP round-trips to **a single streaming request**.

## Motivation

### Where Time Goes Today

In a typical install of a project with ~1000 transitive deps:

| Phase | What happens | Wall clock |
|-------|-------------|-----------|
| **Resolution** | Sequential metadata fetches as tree is discovered depth-first. ~300 unique packages, but tree depth forces sequential batches. | 3-8s |
| **Tarball downloads** | ~50 new/updated packages downloaded in parallel (50 HTTP/1.1 connections) | 2-5s |
| **CAFS extraction** | Decompress tarballs, hash every file, write to store | 1-3s |
| **Linking** | Symlink from store into `node_modules/.pnpm/` | 0.5-1s |

The fundamental problem: **resolution is inherently sequential**. You can't know what `react` depends on until you've fetched `react`'s metadata. Each tree level is a network round-trip. With depth ~10 and ~100ms per request, you're waiting for the tree to unfold one level at a time, even with parallelism within each level.

### What the Client Does Today That a Server Could Do Better

**Resolution** (currently ~300 sequential HTTP requests):
The client fetches package metadata one-at-a-time as it discovers the tree. Each `GET /<package>` returns the full version list (~100ms round-trip). The tree has depth ~10, and each level can only be resolved after the parent level completes. Even with parallelism within a level, this is 5-8 seconds of wall clock time dominated by network latency.

**Tarball download and extraction** (currently ~50 parallel downloads + CPU-bound extraction):
For each new package, the client downloads a `.tgz` tarball, decompresses it, iterates every tar entry, SHA-512 hashes every file, and writes each to the content-addressable store at `files/{digest[:2]}/{digest[2:]}`. The hashing and I/O cost ~1-3 seconds. But the server could pre-compute all of this — it already has every tarball and could pre-extract and pre-hash every file exactly once, amortized across all clients.

**Store indexing** (currently: build `PackageFilesIndex` from scratch per tarball):
After extraction, the client builds a `PackageFilesIndex` — a map of `{relativePath → {digest, size, mode}}` — and writes it to SQLite as a msgpack blob keyed by `"integrity\tpkgId"`. The server could include this index in its response, pre-computed.

## Design

### Core Idea: File-Level Dedup Across the Entire Store

pnpm's content-addressable store stores individual files by SHA-512 digest, not by package. This means:

- `lodash@4.17.20` and `lodash@4.17.21` share 595 out of 600 files → same digests, stored once
- `react` and `react-dom` may share bundled files → same digests
- Thousands of packages share identical `LICENSE` files → one digest

**The server pre-indexes every package version at the file level.** When the client says "I have these packages in my store", the server computes the union of all file digests the client already has. For the packages the client needs, the server checks each file's digest against that set and only sends files with new digests.

This is not package-level dedup (skipping whole packages). This is **file-level dedup across the entire store** — even for packages the client has never seen, most of their files may already exist from other packages.

### The Fetch Protocol in Detail

#### Step 1: Client Reads Its Store State

The client queries its local SQLite store index (`index.db`) to get the list of packages it already has:

```sql
SELECT key FROM package_index
```

Each key is `"integrity\tpkgId"` (e.g., `"sha512-abc...\tregistry.npmjs.org/lodash@4.17.21"`). The client extracts the package IDs.

This is a fast local operation — SQLite with WAL mode, mmap enabled, ~1ms for a few thousand entries.

#### Step 2: Client Sends a Single Request

```
POST /v1/install
Content-Type: application/json
Accept: application/x-pnpm-install
Accept-Encoding: zstd, gzip

{
  // What to resolve
  "dependencies": { "next": "^15.0.0", "react": "^19.0.0" },
  "devDependencies": { "typescript": "^5.0.0" },
  "overrides": {},
  "peerDependencyRules": {},
  "nodeVersion": "22.0.0",
  "os": "linux",
  "arch": "x64",

  // What the client already has (package IDs from store index)
  "storePackages": [
    "registry.npmjs.org/react@19.0.0",
    "registry.npmjs.org/lodash@4.17.21",
    "registry.npmjs.org/typescript@5.7.2",
    // ... typically 500-5000 entries, ~50 bytes each = 25-250KB
  ]
}
```

**Payload size**: For a store with 2000 packages, the `storePackages` list is ~100KB. This is small — fits in a single TCP window after compression.

#### Step 3: Server Resolves and Computes the File Diff

The server performs three operations, all from in-memory data:

**3a. Resolve the dependency tree** (<50ms, often cached):
```
Input:  dependencies + overrides + peerDependencyRules + nodeVersion + os + arch
Output: resolvedPackages = ["/react/19.0.1", "/next/15.1.2", "/typescript/5.7.3", ...]
```

The server has all package metadata in memory (synced from npm's `/_changes` feed). Resolution is pure computation with zero I/O. Results are cached by input hash.

**3b. Compute the client's digest set** (<10ms):
```
clientDigests = Set()
for pkg in request.storePackages:
    clientDigests.addAll(server.fileIndex[pkg].digests)  // pre-indexed
```

The server maintains a pre-computed file index for every package version:
```
server.fileIndex["registry.npmjs.org/react@19.0.0"] = {
  "index.js":     { digest: "a1b2c3...", size: 3421, mode: 0o644 },
  "cjs/react.js": { digest: "d4e5f6...", size: 8192, mode: 0o644 },
  "LICENSE":      { digest: "f7a8b9...", size: 1089, mode: 0o644 },
  // ... ~200 files
}
```

For 2000 store packages averaging 50 files each, this unions ~100K digests. With a pre-built hash set per package, the union is O(n) set merges — fast.

**3c. Compute missing files for needed packages** (<5ms):
```
missingFiles = []
for pkg in resolvedPackages:
    if pkg in request.storePackages:
        continue  // client has the whole package
    for file in server.fileIndex[pkg]:
        if file.digest not in clientDigests:
            missingFiles.append(file)
            clientDigests.add(file.digest)  // dedup within the response too
```

The last line is important: if `react@19.0.1` and `react-dom@19.0.1` both need a new file with the same digest, it's only included once in the response.

#### Step 4: Server Streams the Response

The response is a two-part stream: a JSON header followed by a binary file stream.

```
HTTP/1.1 200 OK
Content-Type: application/x-pnpm-install
Content-Encoding: zstd

┌─────────────────────────────────────────────────────────┐
│ [4 bytes: JSON metadata length (big-endian uint32)]     │
├─────────────────────────────────────────────────────────┤
│ [N bytes: JSON metadata]                                │
├─────────────────────────────────────────────────────────┤
│ [file 1: 64B digest + 4B size + 1B mode + content]     │
│ [file 2: 64B digest + 4B size + 1B mode + content]     │
│ [...]                                                   │
│ [64 zero bytes: end marker]                             │
└─────────────────────────────────────────────────────────┘
```

**JSON metadata** (~2KB per package, ~500KB for 500 packages):

```json
{
  "lockfile": {
    "lockfileVersion": "9.0",
    "importers": { ".": { "dependencies": { "react": "19.0.1" } } },
    "packages": {
      "/react/19.0.1": {
        "resolution": { "integrity": "sha512-..." },
        "engines": { "node": ">=16" }
      }
    }
  },
  "packageFiles": {
    "registry.npmjs.org/react@19.0.1": {
      "integrity": "sha512-xyz...",
      "algo": "sha512",
      "files": {
        "index.js":     { "digest": "a1b2c3...", "size": 3421, "mode": 420 },
        "cjs/react.js": { "digest": "d4e5f6...", "size": 8192, "mode": 420 },
        "LICENSE":      { "digest": "f7a8b9...", "size": 1089, "mode": 420 }
      }
    }
  },
  "missingDigests": ["a1b2c3...", "d4e5f6..."],
  "stats": {
    "totalPackages": 1042,
    "alreadyInStore": 990,
    "packagesToFetch": 52,
    "filesInNewPackages": 3200,
    "filesAlreadyInCafs": 2800,
    "filesToDownload": 400,
    "downloadBytes": 1_200_000
  }
}
```

Note the stats: 52 new packages, but only 400 out of 3200 files actually need downloading — the other 2800 files already exist in the store from other packages.

**Binary file stream** — each entry:

```
┌──────────────────────────────────────────┐
│ digest:  64 bytes (SHA-512, raw binary)  │
│ size:     4 bytes (big-endian uint32)    │
│ mode:     1 byte  (0x00=regular, 0x01=executable) │
│ content: [size] bytes                    │
└──────────────────────────────────────────┘
```

Per-file overhead: 69 bytes. For 400 files, that's 27KB of framing — negligible.

#### Step 5: Client Streams Files Into the Store

The client processes the response as it arrives — no buffering the entire response:

```
1. Read 4-byte JSON length
2. Read and parse JSON metadata
   → Write pnpm-lock.yaml from lockfile data
   → Know the complete file index for every package
3. Stream binary entries:
   For each file:
     a. Read 64-byte digest → hex-encode → compute CAFS path:
        files/{digest[:2]}/{digest[2:]}         (mode=0x00)
        files/{digest[:2]}/{digest[2:]}-exec    (mode=0x01)
     b. Read 4-byte size
     c. Read [size] bytes → write directly to CAFS path
        (atomic: write to temp file, rename)
     d. No hashing needed — server already computed the digest
4. After stream ends (64 zero bytes):
   For each package in packageFiles:
     Build PackageFilesIndex { algo, files: {path → {digest, size, mode}} }
     msgpack-encode → write to index.db as key "integrity\tpkgId"
   (batch all writes in a single SQLite transaction)
5. Symlink node_modules
```

**Why no client-side hashing?** The server is trusted (same as trusting npm's integrity hashes today). The digest in the stream IS the filename in the store — it's self-verifying. If the file is corrupted in transit, the HTTP-level checksum (zstd frame checksums) catches it. For extra paranoia, the client can optionally verify digests, but this is not on the critical path.

### Why This Is the Fastest Possible Approach

| Step eliminated | Time saved | How |
|----------------|-----------|-----|
| Sequential metadata fetches | 3-8s | Server resolves in <50ms with all metadata in memory |
| Tarball decompression (gzip) | 0.5-1s | Files sent uncompressed within a zstd-compressed stream (single decompression, not per-tarball) |
| Tar parsing | 0.2-0.5s | No tar format — files sent with simple length-prefix framing |
| Per-file SHA-512 hashing | 0.5-1s | Server pre-computed digests; client writes to pre-determined paths |
| Downloading redundant files | 1-3s | File-level dedup: only send digests not in client's store |
| Building PackageFilesIndex | 0.1-0.2s | Server sends pre-built indexes in JSON metadata |
| Multiple HTTP connections | 0.5-1s setup | Single connection, single stream |

**Total overhead eliminated: 5-15 seconds for a typical install.**

What remains:
- 1 HTTP round-trip (~100ms)
- Streaming download of only missing file content (~0.5-3s depending on volume)
- Sequential CAFS writes (~0.2-0.5s, pipelined with download)
- SQLite batch write (~10ms)
- Symlink creation (~0.5s)

### Worked Example: Upgrading `react` from 19.0.0 to 19.0.1

**Current pnpm behavior:**
1. Fetch `react` metadata from npm (~100ms)
2. Resolve that 19.0.1 is needed (~1ms)
3. Download `react-19.0.1.tgz` (~200ms, ~300KB compressed)
4. Decompress gzip (~5ms)
5. Parse 200 tar entries (~2ms)
6. SHA-512 hash all 200 files (~20ms)
7. Write 200 files to CAFS — but ~195 already exist (same digest as 19.0.0), so only 5 writes (~5ms)
8. Build PackageFilesIndex, write to SQLite (~2ms)
**Total: ~335ms**, dominated by network

**With pnpm-registry:**
1. Client already sent `storePackages` including `react@19.0.0` in the initial request
2. Server resolves that `react@19.0.1` is needed
3. Server computes: react@19.0.0 has digests {h1..h200}, react@19.0.1 has digests {h1..h195, h196'..h200'} → 5 new digests
4. Server includes 5 files (total ~25KB) in the stream, plus the file index in JSON
5. Client writes 5 files to CAFS (~1ms)
6. Client builds index entry from JSON metadata (~0.5ms)
**Total: ~0ms incremental** (the 5 files are just part of the stream that was already in flight)

### Worked Example: Fresh `create-next-app` Install (Empty Store)

**Current pnpm behavior:**
1. Resolve ~800 packages: ~150 unique metadata fetches, depth ~12 → 5-8s
2. Download ~800 tarballs in parallel (50 connections): ~3-5s
3. Decompress + hash + write ~40K files: ~2-3s
**Total: ~10-16s**

**With pnpm-registry:**
1. Single POST with empty `storePackages` list
2. Server resolves tree (<50ms), computes 40K unique files across 800 packages
3. But many files are shared across packages! Dedup within the response:
   - 40K total file references across 800 packages
   - ~30K unique digests (25% dedup from shared LICENSE, README, polyfills, bundled deps)
   - Response size: ~35MB instead of ~45MB
4. Client streams 30K files to CAFS as they arrive: ~2s (pipelined with download)
5. Batch index write for 800 packages: ~50ms
**Total: ~4-6s** (dominated by download bandwidth)

## Complete Protocol

```
┌────────────────────┐                    ┌─────────────────────┐
│     pnpm CLI       │                    │   pnpm-registry     │
└────────┬───────────┘                    └──────────┬──────────┘
         │                                           │
         │  1. Read store index.db                   │
         │     → storePackages[]                     │
         │                                           │
         │  POST /v1/install                         │
         │  { deps, overrides, peerRules,            │
         │    nodeVersion, os, arch,                  │
         │    storePackages }                         │
         │ ─────────────────────────────────────────>│
         │                                           │
         │                                           │ 2. Resolve tree (<50ms)
         │                                           │ 3. Union storePackages digests
         │                                           │ 4. Diff needed vs. have
         │                                           │ 5. Start streaming response
         │                                           │
         │  ┌─ JSON: lockfile + packageFiles ──────┐ │
         │  │  (client writes pnpm-lock.yaml)      │ │
         │  ├─ File stream ────────────────────────┤ │
         │  │  digest|size|mode|content             │ │
         │  │  digest|size|mode|content             │ │
         │  │  (client writes to CAFS as received) │ │
         │  │  ...                                  │ │
         │  ├─ End marker (64 zero bytes) ─────────┤ │
         │  └──────────────────────────────────────┘ │
         │ <─────────────────────────────────────────│
         │                                           │
         │  6. Batch-write index.db                  │
         │  7. Symlink node_modules                  │
         │                                           │
```

**One HTTP request. Streaming response. Pipelined writes.**

## Expected Performance Gains

| Scenario | Current | With pnpm-registry |
|----------|---------|-------------------|
| Fresh install, empty store (1000 deps) | 15-25s | 5-8s |
| Fresh install, warm store (adding 10 new deps) | 5-10s | 0.5-1s |
| Version upgrade (react 19.0.0 → 19.0.1) | 3-5s | 0.3-0.5s |
| CI, cold cache | 15-25s | 4-6s |
| CI, cached store + lockfile unchanged | 2-3s (frozen) | 2-3s (same — frozen install is already optimal) |
| `pnpm install` after editing package.json | 3-8s | 0.5-1s |

The biggest wins are in:
- **Resolution elimination**: 3-8s → 0.1-0.2s (single round-trip vs. hundreds)
- **File-level dedup**: downloading 400 files instead of 3200 (other 2800 already in store from other packages)
- **No tarball overhead**: no gzip decompression, no tar parsing, no per-file hashing on client
- **Pipelined I/O**: client writes files while still receiving the stream

## Server Architecture

### Pre-Computed File Index

The server's primary data structure — computed once per package version, never recomputed:

```
FileIndex: Map<packageId, PackageFileEntry[]>

PackageFileEntry = {
  relativePath: string      // "lib/index.js"
  digest:       string      // SHA-512 hex, 128 chars
  size:         number      // bytes
  mode:         number      // Unix permission bits
}
```

When a new package version is published to npm:
1. Server downloads the tarball (once)
2. Decompresses, iterates entries, SHA-512 hashes each file
3. Stores the file index permanently
4. Stores the raw file contents keyed by digest (its own CAFS)

This is the exact same work pnpm does on every client today — but done once on the server instead of millions of times across all clients.

### Digest-to-Content Store

The server maintains its own content-addressable store of every file ever published to npm:

```
server-store/
  files/
    a1/b2c3d4...    → file content
    d4/e5f6a7...    → file content
    ...
```

When building a response, the server looks up each missing digest and streams the content directly from its store. No tarball reconstruction needed.

### Resolution Cache

```
ResolutionCache:
  Key:   hash(sortedDeps + overrides + peerConfig + nodeVersion + os + arch)
  Value: complete resolved package list with versions
  TTL:   until any constituent package publishes a new version
```

The cache is invalidated granularly: when `lodash` publishes 4.17.22, only cache entries that include `lodash` in their resolution are invalidated.

### Sync with Upstream npm

The registry doesn't replace npm — it's a layer on top:
- Subscribes to npm registry changes feed (`/_changes`)
- Pre-fetches and pre-indexes every new package version (tarball → file index + content store)
- Resolution cache invalidated when constituent packages change
- Metadata kept in memory for instant resolution

### Private Packages

Two modes:
1. **Proxy mode**: pnpm-registry proxies requests to private registries, caches and indexes their packages the same way
2. **Hybrid mode**: resolve public packages server-side, private packages client-side, merge the graphs

## Architectural Decisions

### Hosted Service vs Self-Hostable

Both. A public `registry.pnpm.io` for open-source packages, and a docker image for enterprises with private packages.

### Lockfile Compatibility

The server returns a lockfile-compatible graph. The client writes it as a normal `pnpm-lock.yaml`. If the pnpm-registry is unavailable, the client falls back to standard resolution and everything still works. The lockfile format doesn't change.

### Server-Side Commands

`pnpm update`, `pnpm dedupe`, and `pnpm why` can also be server-side operations. Send the current lockfile + the command intent, get back the updated graph. The server can run dedupe logic much faster because it has all metadata in memory.

### Hooks (readPackage, afterAllResolved)

These must run client-side. The protocol supports a two-phase flow when hooks are present:
1. Server resolves → client receives lockfile JSON
2. Client applies hooks to the resolved graph
3. If hooks modified anything, client sends modifications back
4. Server re-resolves with constraints and streams files

When no hooks are configured (the common case), the single-request fast path is used.

### Trust Model

The server is trusted to the same degree as npm today. The client already trusts npm to return correct metadata and tarballs. With pnpm-registry:
- Integrity hashes in the lockfile are the same as npm's (`sha512-...`)
- File digests are deterministic (SHA-512 of file content)
- The lockfile is written locally and can be committed/audited
- An optional `--verify-digests` flag can enable client-side SHA-512 verification of every received file

### Backward Compatibility

The pnpm-registry is purely additive. If the config isn't set, pnpm works exactly as it does today. If the registry is down, the client falls back. The lockfile format is unchanged.

Configuration:

```ini
pnpm-registry=https://registry.pnpm.io
```

### Why Package IDs (Not Bloom Filters) for Store State

The client sends package IDs, not a bloom filter of file digests:

| Approach | Payload size (2000 pkgs) | Server computation | Accuracy |
|----------|------------------------|-------------------|----------|
| Package IDs | ~100KB | Union pre-indexed digest sets | Exact |
| Bloom filter of digests | ~120KB | Test each needed digest | False positives (miss files) |
| Full digest list | ~1.6MB | Set lookup | Exact but large |

Package IDs win because:
- Similar payload size to a bloom filter
- No false positives (bloom filter would sometimes claim the client has a file it doesn't, causing a second request to fetch the missed files)
- Server has pre-indexed digest sets per package, so the union is a fast set operation
- Simple to implement on both sides

The one edge case — a partially corrupted store where files were deleted but the index entry remains — is already handled by pnpm's existing integrity verification during linking. If a file is missing from CAFS, the client falls back to fetching it individually.

## Implementation Phases

### Phase 1: Server-Side Resolution Only

Build the registry server with just the resolve endpoint. The client gets the resolved graph in a single request, then downloads tarballs from npm directly (using the existing fetch pipeline). This alone eliminates 3-8s of resolution time.

Changes needed in pnpm CLI:
- New config: `pnpmRegistry: "https://registry.pnpm.io"`
- New resolver that calls the registry instead of doing tree traversal
- Lockfile generation from server response
- Fallback to standard resolution if registry unavailable

### Phase 2: Store-Aware File Streaming

Add the full `/v1/install` endpoint with file-level dedup. The client sends its store package list, the server computes the file diff and streams only missing files in the CAFS-native format described above.

Changes needed:
- Client: read store index, send package list, parse streaming binary response, write to CAFS
- Server: pre-index all packages at file level, compute digest diffs, stream files from server-side CAFS

### Phase 3: Pre-Computed Bundles

For the top 1000 most common dependency trees (next.js, remix, vite, etc.), pre-build compressed file bundles. A fresh `create-next-app` install becomes a single ~20MB download that streams directly into the store — no per-file overhead, no resolution, no diffing.

### Phase 4: Incremental Store Sync

For CI environments that rebuild frequently: the server remembers each client's store state (keyed by a store fingerprint). Instead of the client sending its full package list, it sends a store version hash. The server computes the delta from the last known state.

```
POST /v1/install
{ ..., "storeFingerprint": "abc123", "storeVersion": 47 }

Server knows: version 47 had these packages.
Server computes: you need packages X, Y, Z since then.
Response is even smaller.
```
