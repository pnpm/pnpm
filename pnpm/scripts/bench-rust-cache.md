# Rust cache reuse experiment

Run from the repository root on Linux with Node.js, Cargo, sccache, Git,
GNU `cp`, and a filesystem supporting reflinks:

```sh
just bench-rust-cache
```

The benchmark creates two detached worktrees at `HEAD`. It leaves the current
checkout and existing compiler caches alone. Cargo dependencies must already
be downloaded; builds use `--locked --offline`. If dependencies are missing,
run `cargo fetch --locked` before repeating the benchmark.

The default workload is `pnpm-graph-hasher`. Select another library and its
crate root together:

```sh
just bench-rust-cache --package pnpm-store-dir --source pnpm/crates/store-dir/src/lib.rs --rounds 3
```

Each round compares:

| Mode | Cargo output | Incremental | Compiler cache |
| --- | --- | --- | --- |
| `incremental` | Separate directories | Enabled | None |
| `sccache` | Separate directories | Disabled | One isolated local disk cache |
| `shared-target` | One writable directory | Enabled | None |
| `reflink-snapshot` | Separate directories, B cloned from warm A | Enabled | None |

The sccache mode measures the local tier used by the existing pnpr integration.
It does not measure network transfer or remote cache backfill. Each round
starts with empty build and compiler-cache directories. The host filesystem
cache is not flushed. Mode order rotates between rounds. Cargo configuration
files still apply, including linker and profile settings; wrapper and Rust
flags environment overrides are controlled by the script.

Each mode builds A, repeats A, builds B, edits B, returns to A, and repeats B.
The script appends an observable function to the selected crate root in each
disposable worktree. After every build, it compiles and runs a small consumer
to verify which source revision the library contains. Consumer compilation,
execution, and content scanning are outside the timed Cargo invocation.
An output mismatch is recorded as `correct: false` and printed prominently.
It disqualifies that strategy regardless of its timing.

Results are written after each mode to `results.json`, with Cargo stderr logs
beside it, under `bench-work-env/rust-cache-<timestamp>/`. Use `--output` to
choose a new directory. Build outputs remain available for inspection;
temporary worktrees are removed on normal completion or a caught error.
Terminating the process externally may require manual worktree cleanup.

Compare the median for each phase across rounds. Add `snapshotSeconds` to
`first-b` when comparing the reflink strategy's total restoration cost.
The `compiled` field lists Cargo units reported as not fresh; an sccache hit
still appears there. The separate sccache statistics show compiler cache hits.

Storage is scanned after the first B build. SHA-512 plus the executable bit
matches pnpm CAS's file identity convention. `inodeBytes` counts each hardlinked
file once; `uniqueBytes` counts identical content once; `duplicateBytes` is the
difference. This is an upper bound on file-content deduplication, including
Cargo metadata and incremental files. It excludes CAS manifests and filesystem
metadata. `allocatedBytes` counts allocated blocks per inode and **does not**
account for shared reflink extents. It must not be presented as physical disk
usage for the snapshot strategy. sccache storage is measured separately after
the final build and must be included when assessing that mode's storage cost.

This experiment does not implement a pnpm CAS backend, model Rust build keys,
or make shared Cargo directories safe. A fast snapshot result establishes
only whether preserving and reflinking existing Cargo state merits further
work. Cross-platform restore, concurrent builds, build-script inputs, and
cache retention need separate validation before a product implementation.

## Findings, 2026-09-06

Measured revision `ad0b50d5ed16a7224478bd4c8d4843338926df6e` on Linux x86-64,
Btrfs, a Ryzen 9 9950X3D2 with 32 logical CPUs, Rust 1.97.0, and sccache 0.17.0.
The host Cargo config selected Clang as linker and `line-tables-only` dev
debug information. These are exploratory measurements on a developer machine,
not results from a dedicated performance runner.

The small workload is the median of three rounds of `pnpm-graph-hasher`
(24 Cargo artifacts). The larger workload is a confirmation run of
`pnpm-store-dir` (182 artifacts). A preceding three-round larger run reproduced
the correctness results, but overlapped other compilation during some phases,
so its timings are omitted here. Snapshot times below include the reflink copy.

| Strategy | Small: first B | Small: edit B | Larger: first B | Larger: edit B | Output checks |
| --- | ---: | ---: | ---: | ---: | --- |
| Separate incremental builds | 1.43 s | 0.15 s | 4.75 s | 0.30 s | Passed |
| Shared local sccache | 1.70 s | 0.23 s | 5.71 s | 0.74 s | Passed |
| Shared writable target | Disqualified | Disqualified | Disqualified | Disqualified | A returned B's edited code |
| Independent reflink snapshot | 0.20 s | 0.24 s | 0.71 s | 0.83 s | Passed |

The shared writable target failed the return-to-A check in every tested round.
Cargo reported the unit as fresh, but the linked consumer returned `1` when
A's source returned `0`. Sharing a mutable target directory between independent
worktrees is therefore not a suitable implementation for this feature. Cargo's
unit freshness state does not give these checkouts independent ownership of the
output slot. A shared store must publish immutable entries and materialize an
independent writable build directory for each checkout.

The reflink snapshot reduced second-worktree startup by about 7 times in these
workloads. It did not speed up the first subsequent edit. That edit was slower
than editing an existing local incremental build. The experiment establishes
a worktree-startup opportunity, not an improvement to the steady edit loop.

The larger pair of independently built incremental directories contained
1,166,904,459 bytes after counting hardlinks once. Unique content occupied
707,213,087 bytes. The remaining 459,691,372 bytes, about 438 MiB or 39%, is an
upper bound on additional content deduplication. This does not count manifest
overhead, and some duplicate extents may already be shared by the filesystem.
It is not a measured reduction in physical disk allocation.

sccache recorded zero Rust hits in both workloads. The larger workload did
have three native compiler hits. The different absolute compilation paths
prevented useful Rust reuse with this setup, consistent with the existing
[compiler-cache integration documentation](../../pnpr/crates/pnpr/README.md#cargo-compilation-cache).
Improving path-compatible cache reuse is a separate opportunity from CAS
deduplication. Relocating the cache directory alone does not address it.

Next, prototype immutable snapshots in a disposable project-build cache with
clone-or-copy materialization, preserving per-worktree mutable state. Reuse
CAS primitives without sharing the installed-package store's lifetime.
Measure publication,
manifest lookup, hashing, restoration, and the first edit together. Snapshot
selection must include source identity and the compilation environment;
the npm dependency-graph key or Cargo.lock alone is insufficient. Do not expose
the shared writable target strategy as a user feature.

Raw local records for this investigation:

- `bench-work-env/rust-cache-1788720139306/results.json` (small, three rounds)
- `bench-work-env/rust-cache-1788720193292/results.json` (larger exploratory run)
- `bench-work-env/rust-cache-1788720328535/results.json` (larger confirmation)

These generated directories are ignored by Git. Running the command above
produces fresh records with the same schema.

## Fit with local-project build caching

The product boundary is a workspace task that builds a local project, including
a task that invokes Cargo. A snapshot would preserve that task's incremental
state between worktrees. This does not require pnpm to manage arbitrary Cargo
invocations outside its task runner.

There are two different cache contracts:

| Artifact | On a hit | Required identity |
| --- | --- | --- |
| Completed task outputs | Restore outputs and skip execution | All task inputs, including upstream task inputs or outputs and the build environment |
| Incremental build state | Restore private state, then execute the task | A validated snapshot compatibility model plus source identity; never treat a state hit as task completion |

Start with exact-source snapshots. Reusing a snapshot from an older revision
is a separate capability that needs invalidation tests. Always running Cargo
is necessary for state restoration, but is not sufficient proof of freshness:
the shared-target experiment already produced incorrect output after Cargo ran.

### What this checkout already provides

These findings describe `ad0b50d5ed16a7224478bd4c8d4843338926df6e`.
The local-project cache feature mentioned in the discussion is implemented in
the open proof-of-concept [pnpm/pnpm PR 14233](https://github.com/pnpm/pnpm/pull/14233),
on `pnpm-ci-poc`, not in this checkout. Its implementation is described below.

- [`run.rs`](../crates/cli/src/cli_args/run.rs) and its recursive runner execute
  workspace scripts. [`task_run_state.rs`](https://github.com/pnpm/pnpm/blob/ad0b50d5ed16a7224478bd4c8d4843338926df6e/pnpm/crates/cli/src/cli_args/task_run_state.rs)
  records task execution for resumption; its invocation identity is not a hash
  of source-file contents and cannot serve as a build-cache key.
- [`shared-artifact-protocol`](../crates/shared-artifact-protocol/src/lib.rs)
  already defines `WorkspaceTask { project, task }`, `workspace-task:v1` artifacts,
  and organization ownership. The pnpr
  [store tests](../../pnpr/crates/shared-artifacts/src/tests.rs) include
  `workspace_task_subjects_round_trip_through_the_store`. This is an existing
  remote storage integration point, not evidence of a complete CLI task cache.
- [`shared_side_effects.rs`](../crates/deps-restorer/src/shared_side_effects.rs)
  connects installation to signed dependency artifacts. Its package integrity,
  npm graph keys, Node platform selection, and store-index updates are specific
  to dependencies. Reuse protocol/client capabilities, not this install adapter.
- [`link_file.rs`](../crates/deps-restorer/src/link_file.rs) has clone-or-copy
  handling and filesystem fallback tests. Extract reusable filesystem behavior
  if a task cache needs it; do not make task execution depend on dependency
  restoration. The macOS
  [`dir_clone_cache`](../crates/deps-restorer/src/dir_clone_cache.rs) shares GVS
  slots, so its storage ownership is unsuitable for a separately disposable cache.
- The [sccache service](../../pnpr/crates/pnpr/README.md#cargo-compilation-cache)
  shares pnpr artifact infrastructure but uses a separate compiler namespace.
  It caches compiler results, not Cargo incremental directories. A task snapshot
  should not masquerade as an sccache entry or a dependency-side-effects artifact.

### Cache removal is part of the contract

Use a dedicated build-cache root outside the package store and GVS. Reusing
hashing, blob storage, and cloning code must not make installed packages depend
on this root. Cross-store blob deduplication is outside the initial prototype.

Published snapshots are immutable. Each task receives independent writable
files through reflinks or copies, never hardlinks to mutable files or symlinks
back into the cache. Removing the entire cache must leave restored build
directories and installed dependencies usable. The only subsequent effect is
a cache miss and rebuilding, assuming the normal build inputs are available.

Restore into staging and expose the completed directory only after validation.
A missing blob or concurrent eviction discards staging and becomes a miss.
Do not overwrite an existing live target directory, and do not snapshot it
while Cargo or another writer is changing it. Publication needs exclusive
ownership of the build directory and a check that inputs remained stable.
For multiple workspace tasks sharing a Cargo target, ownership belongs to that
target directory, not merely to the npm package or task name.

### Cargo-specific gaps

Snapshot selection must account for source contents, local path dependencies,
Cargo manifests and lockfile, effective Cargo configuration, Cargo/rustc
versions, host and target, profile, features, flags, linker/native tools, and
declared build-script or procedural-macro inputs and environment. A Git commit
alone misses dirty and untracked inputs. Unknown external inputs should exclude
a task from caching until it has an explicit input contract.

Cargo's [fingerprint documentation](https://doc.rust-lang.org/stable/nightly-rustc/cargo/core/compiler/fingerprint/index.html)
describes timestamp-based freshness checks. Its
[build-script documentation](https://doc.rust-lang.org/cargo/reference/build-scripts.html#change-detection)
also describes file and environment change tracking. Restoring bytes and modes
alone is not equivalent to the benchmark's `cp --archive`: snapshot metadata,
source timestamps, and absolute paths need a defined restoration policy.
Preserving all timestamps blindly is not a correctness solution either.

The current shared artifact manifest contains path, integrity, mode, and size,
but no timestamps or symlink representation. It caps each file and the total
artifact at 64 MiB, with at most 10,000 files. The measured larger pair has
about 707 MB of unique content, so whole-target remote reuse cannot assume this
protocol's limits are adequate. Measure each snapshot and its largest files.
Its current platform tag helpers also describe Node platforms, not Rust
toolchain compatibility. A Rust state format needs an explicit version and
compatibility rules, distinct from completed task outputs. Avoid raising shared
limits or marking native artifacts universal just to make a prototype fit.

### Bounded next experiment

Implement a local-only snapshot lifecycle in the benchmark before adding CLI
settings or remote publication. Use a disposable cache root, an explicit fixture
input manifest, and independent target directories. Exercise the same lifecycle
that a workspace task would use: lookup, restore, execute, publish. Keep normal
existing local incremental builds as the baseline.

Required checks:

1. Restore B, delete the entire snapshot cache, then run and edit both A and B.
   Verify outputs and confirm installed dependencies remain usable.
2. Edit B after restoration and verify neither A nor the published snapshot
   changes. Test reflink and forced-copy restoration separately.
3. Delete a snapshot or blob during restoration and verify a clean miss with
   no partial target exposed. Test concurrent publication and interrupted writes.
4. Change source bytes while retaining size and timestamp, switch branches,
   change a path dependency, and change build-script file/environment inputs.
   Compare executable output with an independent clean build each time.
5. Change profiles, features, compiler flags, target paths, and toolchain identity.
   Incompatible snapshots must be rejected. Include build scripts and native
   compilation, beyond the existing library probe.
6. Measure lookup, hashing, publication, restoration, first build, and first edit
   together. Report snapshot bytes and file counts alongside timings; the
   existing startup speedup excludes most cache lifecycle costs.

If these checks pass and total cost remains useful, connect the lifecycle to
the local-project task cache from PR 14233. Remote support
then builds on workspace-task artifact infrastructure with a reviewed Rust
state format, bounded transfer, and trusted publication. No new v11 feature
is proposed.

### Connection to the pipeline proof of concept

Inspected PR 14233 at `02289dd385955818727b350795d606c46233a19d`.
Its [`pipeline/cache.rs`](https://github.com/pnpm/pnpm/blob/02289dd385955818727b350795d606c46233a19d/pnpm/crates/cli/src/cli_args/pipeline/cache.rs)
implements task keys, output collection, copy-based storage/restoration, and
captured logs. The
[`pipeline.rs`](https://github.com/pnpm/pnpm/blob/02289dd385955818727b350795d606c46233a19d/pnpm/crates/cli/src/cli_args/pipeline.rs)
caller skips execution on a successful restore and replays those logs. This is
the completed-task-output contract above and is a concrete place to integrate
incremental state around task execution on an output-cache miss.

Its task artifacts live under
`<cacheDir>/pipeline/<hash-of-workspace-path>/tasks`; restoration records and run
reports are siblings. Output bytes are copied into the working tree, so removing
the cache does not unlink restored outputs. Losing restoration records makes
later restores more conservative about overwriting existing files. Deleting the
whole pipeline directory also loses local reports, which is separate from
whether installed projects or built binaries keep working.

Three changes are needed before this can deliver the worktree reuse measured here:

- Separate shared task artifacts from workspace-local restoration records.
  The current workspace-path namespace prevents A and B from finding the same
  entry even when their task keys match. Shared keys need repository identity
  and an appropriate trust scope; removing the workspace prefix alone is not
  a complete design.
- Add optional incremental-state restoration on a completed-output miss, then
  execute the task normally. Keep final outputs and state snapshots separate;
  declaring `target/**` as outputs would use the existing skip-execution contract
  and would not implement incremental reuse after a source edit.
- Extend input/environment identity for Cargo. The PoC hashes script bodies,
  declared environment variables, selected project files, upstream task keys,
  the pnpm lockfile, and Node's version. It includes untracked, unignored files,
  but input globs only filter that enumerated project file set. External Cargo
  configuration, ignored build inputs, sibling path dependencies, and the Rust
  toolchain need explicit coverage. Its ordinary `fs::copy` restoration also
  needs the Cargo metadata and freshness validation described above.

The PR has no remote task-cache tier. Its pnpr integration publishes run records,
not task outputs. The workspace-task artifact protocol in this checkout is a
candidate for a later remote tier; it is not wired into the PoC cache. Reuse the
task-cache lifecycle from the PoC rather than adding an unrelated Cargo command,
while keeping Rust state compatibility and disposal independently testable.

## Prototype on the pipeline branch

`feat/pipeline-cargo-cache` starts from PR 14233 at
`02289dd385955818727b350795d606c46233a19d`. See
[pipeline-cargo-cache.md](pipeline-cargo-cache.md) for the opt-in task setting
and supported input contract.

The prototype stores immutable directory snapshots in a dedicated disposable
cache shared by linked Git worktrees. It restores only an absent target, always
runs the task, and retains independent writable files. It does not implement
CAS deduplication or remote transfers. The integration test runs a real Cargo
build in two worktrees, edits source without advancing its timestamp, and
verifies both binaries before and after deleting the cache.

A build-script relocation test exposed another correctness failure beyond the
original shared-target experiment: an independent snapshot could retain the
publishing worktree's path in a generated build-script value after Cargo ran.
The prototype invalidates local-package and build-script freshness records on
restoration. Source input changes invalidate all freshness records while
retaining incremental files. The relocation regression now verifies that each
binary contains its own worktree path.

The original benchmark's startup measurements do not include this invalidation,
repository input hashing, snapshot integrity checks, or publication. They must
not be used as measured speedups for the pipeline implementation.
