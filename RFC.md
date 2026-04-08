# RFC: pnpm-registry — Server-Side Resolution and Store-Aware Downloads

## Summary

A pnpm-specific registry layer that runs dependency resolution server-side and uses knowledge of the client's content-addressable store to deliver only the exact files that are missing. This reduces a typical install from dozens of sequential HTTP round-trips to **two requests**: one to resolve, one to download.

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

## Design

### Three Core Optimizations

#### 1. Server-Side Resolution (biggest win)

The client sends its dependency requirements in **one request**. The server resolves the entire tree and returns a complete lockfile-equivalent.

```
Client → Server:
POST /v1/resolve
{
  "dependencies": { "next": "^15.0.0", "react": "^19.0.0" },
  "devDependencies": { "typescript": "^5.0.0" },
  "overrides": { ... },
  "peerDependencyRules": { ... },
  "nodeVersion": "22.0.0",
  "os": "linux",
  "arch": "x64"
}

Server → Client:
{
  "resolvedGraph": {
    "/next/15.1.2": {
      "resolution": { "integrity": "sha512-..." },
      "dependencies": { "react": "/react/19.0.1", ... },
      "files": {
        "dist/index.js": "sha512-abc...",
        "dist/server.js": "sha512-def...",
        ...
      }
    },
    "/react/19.0.1": { ... },
    ...
  }
}
```

**Why this is fast**: The server has ALL metadata in memory. Resolution is pure computation — no network I/O, no sequential HTTP round-trips. A 1000-package tree resolves in <50ms server-side. The client gets the full graph in a single HTTP round-trip (~100-200ms).

**Server caching**: The server can cache resolved subgraphs. The resolution of `react@^19.0.0` is the same for every project — the server computes it once and reuses it. This is essentially a CDN for resolution.

#### 2. Store-Aware Downloads (eliminates redundant data)

The client tells the server what it already has. The server responds with only what's missing.

**Option A — Package-level (simple, v1)**:

```
Client → Server:
POST /v1/download-plan
{
  "have": ["/react/19.0.0", "/next/15.1.1"],
  "need": ["/react/19.0.1", "/next/15.1.2"]
}

Server → Client:
{
  "download": ["/react/19.0.1", "/next/15.1.2"],
  "bundleUrl": "/v1/bundle/session-xyz",
  "bundleSize": 2_400_000
}
```

**Option B — File-level (powerful, v2)**:

```
Client → Server:
POST /v1/download-plan
{
  "need": {
    "/react/19.0.1": ["sha512-abc...", "sha512-def...", ...],
    "/next/15.1.2": [...]
  },
  "haveHashes": "<bloom filter of all file hashes in store>"
}

Server → Client:
// Binary stream of only the files whose hashes aren't in the bloom filter
// Pre-formatted for direct CAFS insertion (no tarball decompression needed)
```

This is where pnpm's content-addressable store pays off massively. When upgrading `react` from 19.0.0 to 19.0.1, maybe 5 out of 200 files changed. File-level dedup means downloading 50KB instead of 2MB.

#### 3. CAFS-Native Wire Format (eliminates extraction)

Currently: download `.tgz` → decompress → iterate entries → hash each file → write to store.

With a pnpm-native registry: the server pre-extracts packages and stores the file index. The download stream is already organized by content hash:

```
[4 bytes: hash length][hash][4 bytes: file size][file content][1 byte: executable flag]
[4 bytes: hash length][hash][4 bytes: file size][file content][1 byte: executable flag]
...
```

The client writes directly to `files/{exec|nonexec}/{hash[:2]}/{hash[2:]}` — no decompression, no tar parsing, no rehashing.

## Complete Protocol: One Install in Two Requests

```
┌────────────────────┐                    ┌─────────────────────┐
│     pnpm CLI       │                    │   pnpm-registry     │
└────────┬───────────┘                    └──────────┬──────────┘
         │                                           │
         │  POST /v1/resolve-and-plan                │
         │  {                                        │
         │    deps, devDeps, overrides,              │
         │    nodeVersion, os, arch,                 │
         │    currentLockfileHash,                   │
         │    storePackages: ["/react/19.0.0", ...], │
         │    // OR storeBloomFilter: "base64..."    │
         │  }                                        │
         │ ─────────────────────────────────────────>│
         │                                           │ resolve tree (cached, <50ms)
         │                                           │ diff against client store
         │                                           │ build download bundle
         │  {                                        │
         │    lockfile: { ... },                     │
         │    downloadUrl: "/v1/bundle/abc123",      │
         │    bundleSize: 3_200_000,                 │
         │    stats: { total: 1042, cached: 990,     │
         │             download: 52 }                │
         │  }                                        │
         │ <─────────────────────────────────────────│
         │                                           │
         │  GET /v1/bundle/abc123                    │
         │ ─────────────────────────────────────────>│
         │                                           │
         │  [binary stream: CAFS-native files]       │
         │ <─────────────────────────────────────────│
         │                                           │
         │  write files to store                     │
         │  update index.db                          │
         │  symlink node_modules                     │
         │                                           │
```

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
- **Resolution elimination**: 3-8s → 0.1-0.2s (single round-trip)
- **Warm store upgrades**: downloading 50KB of changed files instead of 5MB of tarballs
- **CI without store cache**: server caches the resolution, client just downloads

## Server Architecture

### Data Structures

```
PackageIndex:
  For every version of every package:
    - resolved manifest (dependencies, peerDependencies, etc.)
    - file index: Map<relativePath, {hash: string, size: number, executable: boolean}>

ResolutionCache:
  Key: hash(sortedDeps + overrides + peerConfig + nodeVersion + os + arch)
  Value: complete resolved graph
  TTL: until any constituent package publishes a new version

PrebuiltBundles:
  For common dependency combinations (next@15 + react@19, etc.):
    pre-built download bundles ready to serve
```

### Sync with Upstream npm

The registry doesn't replace npm — it's a layer on top:
- Subscribes to npm registry changes feed (`/_changes`)
- Pre-fetches and pre-indexes every new package version
- Stores file-level CAFS index for each
- Resolution cache invalidated when constituent packages change

### Private Packages

Two modes:
1. **Proxy mode**: pnpm-registry proxies requests to private registries, caches metadata
2. **Hybrid mode**: resolve public packages server-side, private packages client-side, merge graphs

## Architectural Decisions

### Hosted Service vs Self-Hostable

Both. A public `registry.pnpm.io` for open-source packages, and a docker image for enterprises with private packages.

### Lockfile Compatibility

The server returns a lockfile-compatible graph. The client writes it as a normal `pnpm-lock.yaml`. If the pnpm-registry is unavailable, the client falls back to standard resolution and everything still works. The lockfile format doesn't change.

### Server-Side Commands

`pnpm update`, `pnpm dedupe`, and `pnpm why` can also be server-side operations. Send the current lockfile + the command intent, get back the updated graph. The server can run dedupe logic much faster because it has all metadata in memory.

### Hooks (readPackage, afterAllResolved)

These must run client-side. The protocol supports: server resolves → client applies hooks → if hooks modified anything, client tells server → server re-resolves with modifications.

### Backward Compatibility

The pnpm-registry is purely additive. If the config isn't set, pnpm works exactly as it does today. If the registry is down, the client falls back. The lockfile format is unchanged.

Configuration:

```ini
pnpm-registry=https://registry.pnpm.io
```

## Implementation Phases

### Phase 1: Server-Side Resolution

Build the registry server with just the resolve endpoint. This alone gives the biggest win (eliminating sequential round-trips). The client still downloads tarballs from npm directly, but knows exactly what to download from a single request.

Changes needed in pnpm CLI:
- New config: `pnpmRegistry: "https://registry.pnpm.io"`
- New resolver that calls the registry instead of doing tree traversal
- Lockfile generation from server response
- Fallback to standard resolution if registry unavailable

### Phase 2: Store-Aware Downloads (Package-Level)

Add the download-plan endpoint. Client sends list of packages in store, server returns a single bundle URL. Client downloads one file instead of N tarballs.

### Phase 3: File-Level Dedup

Pre-index all packages at the file level. Client sends bloom filter of store hashes, server sends only missing files in CAFS-native format. This is where version upgrades become near-instant.

### Phase 4: Pre-Computed Bundles

For the top 1000 most common dependency combinations, pre-build download bundles. A fresh `create-next-app` install could be a single 20MB download that goes straight into the store.
