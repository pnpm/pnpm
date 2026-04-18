---
"@pnpm/config.reader": minor
"@pnpm/worker": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/store.cafs": minor
"@pnpm/store.cafs-types": minor
"@pnpm/store.create-cafs-store": patch
"@pnpm/store.controller": patch
"@pnpm/installing.package-requester": patch
"@pnpm/lockfile.fs": minor
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": minor
---

Parallelized and async I/O improvements across the install pipeline for significantly faster installs:

- **Concurrent project building**: `projectsToInstall` processing now runs in parallel with configurable concurrency (up to `os.availableParallelism()` or 16, whichever is lower)
- **Parallelized symlink creation**: `symlinkAllModules` uses p-limit concurrency for concurrent symlink operations
- **Async integrity verification**: Package file integrity checks now use async I/O (streaming reads + async stat) instead of synchronous operations
- **Async directory scanning**: `addFilesFromDir` uses async `node:fs/promises` for non-blocking file system traversal
- **Optimized lockfile writes**: Skip writing lockfile when unchanged; concurrent file writes for lockfile generation
- **Concurrency utility**: Added `availableParallelism()` helper for automatic CPU-aware concurrency sizing
