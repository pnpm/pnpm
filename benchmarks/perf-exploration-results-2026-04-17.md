# Performance exploration results — 2026-04-17

## Scope

This document records the reconciliation of PR #11286 with maintainer feedback and prior pnpm performance work.

## Maintainer feedback incorporated

Maintainer feedback on the draft PR pointed to two prior upstream efforts:

- `#10886` — switched npm registry metadata cache from msgpack to JSON because msgpack was slower for package document cache workloads.
- `#10827` — explored SQLite + msgpack for store/package metadata and found mixed results, with hot-path wins in some cases but also regressions/noise elsewhere. Follow-up comments in that PR noted that shared msgpack structures were slower and that broad claims needed end-to-end benchmark validation.

Based on that feedback, the experimental registry metadata cache added in this branch was removed pending stronger end-to-end evidence.

## Changes made after feedback

Removed from this branch:

- `resolving/registry-metadata-cache/`
- `resolving/npm-resolver/test/binaryCache.benchmark.ts`
- `resolving/npm-resolver/tsconfig.benchmark.json`
- `resolving/npm-resolver/benchmark-out/binaryCache.benchmark.js`
- `@pnpm/resolving.registry-metadata-cache` integration from `resolving/npm-resolver`
- Changeset `.changeset/registry-metadata-cache.md`

Retained in this branch:

- Platform-specific directory cloning (`cloneDir`) in `fs/indexed-pkg-importer`
- Adaptive/default workspace concurrency changes in `config/reader`
- Existing JSON/LRU-related work that is independent of the removed registry metadata cache experiment

## Focused verification run

After removing the registry metadata cache work, the following focused verification was run locally.

### 1. cloneDir tests

Command:

```sh
NODE_OPTIONS="--experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169"   pnpm --filter @pnpm/fs.indexed-pkg-importer exec jest fs/indexed-pkg-importer/test/cloneDir.test.ts --runInBand
```

Result:

- PASS
- 5 tests passed
- 2 Linux-only tests skipped on macOS

### 2. concurrency tests

Command:

```sh
NODE_OPTIONS="--experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169"   pnpm --filter @pnpm/config.reader exec jest config/reader/test/concurrency.test.ts --runInBand
```

Result:

- PASS
- 8 tests passed

### 3. lockfile sync

Command:

```sh
pnpm install --no-frozen-lockfile
```

Result:

- PASS
- lockfile/catalog state refreshed after removing the registry metadata cache package

## Benchmark conclusion

No new end-to-end benchmark run currently justifies keeping the registry metadata cache experiment in this branch.

The benchmark-backed conclusion for this branch is therefore:

- Keep the cloneDir optimization candidate.
- Keep the concurrency/default-worker tuning candidate.
- Remove the registry metadata cache experiment until a full benchmark pass using `benchmarks/bench.sh` demonstrates a meaningful end-to-end win on pnpm install scenarios.

## Next recommended benchmark pass

Before reintroducing any registry metadata cache experiment, run:

```sh
pnpm run compile
./benchmarks/bench.sh
```

And compare at minimum these scenarios from `benchmarks/bench.sh`:

- Headless (warm store+cache)
- Re-resolution (add dep, warm)
- Full resolution (warm, no lockfile)
- Headless (cold store+cache)
- Cold install (nothing warm)
- GVS warm reinstall

Only restore the registry metadata cache work if those end-to-end numbers show a clear win rather than isolated microbenchmark gains.

## Additional verification

### 4. npm resolver suite

Command:

```sh
NODE_OPTIONS="--experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169" \n  pnpm --filter @pnpm/resolving.npm-resolver exec jest --runInBand
```

Result:

- PASS
- 11 test suites passed
- 140 tests passed

## Notes

A targeted `pnpm exec tsgo --build resolving/npm-resolver fs/indexed-pkg-importer config/reader` compile check also surfaced pre-existing or out-of-scope type inconsistencies in the broader branch around async CAFS/store APIs (`store/create-cafs-store` and `worker/src/start.ts`). Those are separate from the registry metadata cache removal itself, so this document records the focused verification results for the retained optimizations rather than claiming a clean whole-branch compile.
