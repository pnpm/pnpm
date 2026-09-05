# Ecosystem install lifecycle

The npm, Cargo and Python integrations share operation ownership, not dependency
semantics. `pnpm-install-coordinator` contains the install lifecycle without a
dependency on CLI arguments, configuration, a resolver, or an ecosystem enum.
The CLI chooses participants. Each participant declares its metadata footprint
and returns a deferred `InstallTask`.

```text
CLI selection -> native plans + npm task
                         |
             lock workspace, snapshot metadata
                         |
              prepare all tasks concurrently
                         |
                  settle every task
                    /           \
                 error         success
                   |              |
            restore metadata   publish projections
                                  /       \
                               error     success
                                 |          |
                       reverse publication  retain resources
                       restore metadata
```

## Ownership

| Component | Owns |
| --- | --- |
| CLI registration | Enabled ecosystems, command option translation, shared HTTP context |
| Native task | Discovery interpretation, manifests, resolution, native lockfile bytes, verified projection preparation |
| `InstallPlan` | Workspace lock, metadata snapshots, concurrent preparation, settlement barrier, publication order, rollback |
| `PreparedInstall` | Publishing and undoing its non-metadata projection, retaining published resources |
| Network and store crates | Authentication routes, network budget, verified artifacts and content-addressed files |

`install` and mixed `add` use the same lifecycle. Their differences are inputs
to native plans, not extra coordinator branches. Adding another ecosystem does
not require editing the lifecycle crate or teaching it a manifest basename.
Cargo and Python declare their own metadata paths. The npm command declares
its configured manifest, lockfile, modules-state and workspace-settings paths
when participating in mixed add.

The shared inventory excludes configured store, cache, state, modules and
virtual-store directories before descending into them. Managed storage is not
workspace input, even when it contains native manifests and lives inside the
workspace. Exclusions are paths, so real projects with the same directory
basename elsewhere remain discoverable.

Preparation starts only after the lock and all metadata snapshots exist.
Every task must settle its spawned work before returning, including error
paths. The coordinator waits for every task even after a sibling fails.
Dropping a prepared result releases unpublished temporary resources.

Publication follows task registration order. Cargo sorts prepared workspaces
by root so download timing does not change publication order. If publication
fails, the coordinator rolls back every attempted projection in reverse order,
including the failing publisher. Metadata rollback follows projection rollback.
Rollback continues after an error. A rollback failure retains resources for
manual recovery and reports both publication and rollback errors.

## Native projections and limits

Cargo prepares verified versioned store slots and native lockfile contents.
Its workspace source links, managed source configuration and lockfile publish
after all participants prepare successfully. Source links are additive cache
entries; metadata rollback restores the selected graph, not cache contents.

Python prepares a complete interpreter-specific environment generation. Its
publication switches the managed environment link and writes `pylock.toml`.
Rollback restores the previous link. Old successful generations remain alive
for already-running processes. Interpreter and platform identity stay inside
Python's lockfile freshness and wheel-selection logic.

npm explicitly enrolls as an in-place installer. Its existing materialization
and lifecycle-script behavior remains intact; mixed add supplies metadata for
rollback, but this does not make `node_modules` transactional. Ordinary install
does not roll back npm metadata. npm lifecycle scripts are not scheduled after
the other ecosystems' publication and must not assume those projections are
already available. A future cross-ecosystem build graph needs a separate
post-publication phase, not another meaning for preparation.

This is an internal, in-process contract, not a public plugin API or crash
journal. Process termination and externally cancelled operations do not gain
crash recovery. Immutable artifacts, caches and unused source links may survive
failure. Generation garbage collection is a separate concern.

## Performance and verification

The npm-only path returns before creating a plan or inventory. No task objects,
metadata snapshots, workspace locks or native manifest discovery are added to
that path. Dynamic dispatch happens per prepared project, not per package,
resolver candidate or file.

Coordinator tests exercise concurrency, settlement before metadata rollback,
publication barriers, reverse rollback, temporary-resource cleanup and resource
retention after rollback failure. Existing descriptor-relative metadata tests
run in the lifecycle crate. CLI tests exercise real npm/Cargo/Python installs,
native lockfile interoperability, failing preparation and failing publication.

The Python integration's initial benchmark evidence remains in
[PYTHON_SPIKE.md](./PYTHON_SPIKE.md). The lifecycle refactor is measured separately
against the integrated implementation so these are distinct comparisons.

The final Linux release build was compared with the integrated baseline
`8769175403`, with matching build features and an unchanged pnpr binary.
All eleven scenarios passed. Each target received four warmups and 30
measurements, with the refactor target running first.

| npm-only scenario | Baseline mean (ms) | Refactor mean (ms) |
| --- | ---: | ---: |
| Fresh install, cold cache/store | 492.12 | 487.22 |
| Fresh install, hot cache/store | 242.76 | 247.36 |
| Fresh install, cold cache/hot store | 427.43 | 428.17 |
| Frozen restore, cold cache/store | 375.55 | 375.10 |
| Frozen restore, hot cache/store | 108.35 | 108.68 |
| Repeat install, hot cache/store | 6.19 | 6.03 |
| Repeat install, cold cache/hot store | 5.98 | 6.03 |
| Add, hot cache/store | 261.20 | 259.68 |
| Offline resolution, hot cache | 164.48 | 164.38 |
| GVS frozen restore, hot cache/store | 103.21 | 103.72 |
| Frozen restore, cold cache/store/pnpr | 429.49 | 428.52 |

An earlier 15-sample run before the discovery fix showed 3.5% slower add
and 4.2% slower cold-cache repeat installs. These did not reproduce in the
reversed-order final run: add was 0.6% faster and repeat install 0.7% slower.
The measurements do not establish a speedup.

The final run's largest slowdown, 1.9% for hot-cache/store fresh installs,
was checked again with the baseline first and 40 measurements per target.
It measured 246.46 ms versus 246.85 ms (standard deviations 4.20 and 4.82 ms),
a 0.2% difference. The larger slowdown did not reproduce.
