# Lockfile-resolution reuse (pacquet)

Port pnpm's behavior: during a non-frozen install, **reuse the prior lockfile's
resolution + transitive subtree** for dependencies that are still satisfied and
not being updated, instead of re-resolving everything from manifests (pacquet
today only feeds the lockfile into preferred-versions seeding). This closes the
perf gap that the merged tarball warm-store-reuse PR (#12096) only patched for
remote tarballs, and matches how pnpm avoids re-resolving unchanged trees.

## pnpm reference (source of truth)

`installing/deps-resolver/src/resolveDependencies.ts`:
- `getInfoFromLockfile(lockfile, registries, reference, alias)` (~L1199) — look up
  the recorded snapshot for an alias's ref; returns resolution + `dependencyLockfile`
  (the transitive child refs).
- Reuse predicate in `resolveDependenciesOfDependency` (~L844-881): `update = false`
  unless update-requested, the snapshot is missing (new dep), a workspace pkg became
  available, or the parent is in `updatedSet`.
- `getDepsToResolve` (~L1086) matches each wanted child against `resolvedDependencies[alias]`
  via `satisfiesWanted` (semver-satisfies, not string-equality).
- Subtree propagation in `resolveChildren` (~L1000): `resolvedDependencies =
  parentPkg.updated ? undefined : currentResolvedDependencies` — an unchanged parent
  feeds its lockfile child-refs down; an updated parent discards them, forcing the
  whole subtree to re-resolve.
- `packageRequester.ts` (~L155-277): on `update=false` the request returns
  `updated:false` and skips fetch.

## Key simplification for pacquet

A given package **version's dependency set is immutable**, and the lockfile snapshot
already reflects any `readPackageHook`/`packageExtensions` that were applied when it
was written. So for a reused parent version, its transitive subtree is exactly the
snapshot's recorded child-refs — we can **walk the snapshot subtree (frozen-install
style) instead of re-resolving from the parent manifest**, and need neither the
parent's package.json nor its child *ranges*. `install_frozen_lockfile.rs` already
performs this snapshot→graph walk and is the reusable building block.

A *changed* `readPackageHook`/`packageExtensions` config invalidates reuse: the install
withholds the prior lockfile from the reuse path when its `packageExtensionsChecksum` no
longer matches the config, so the stale subtree is re-resolved (mirrors pnpm invalidating
the lockfile on a settings change). `overrides` drift is not yet guarded for transitive
reuse — see follow-ups.

## Design: hybrid resolve

Fresh-resolve new/changed/update-targeted deps + their subtrees through the existing
`resolve_node` path; snapshot-walk the unchanged subtrees; merge into one
`DependenciesGraph`. The reuse decision threads down the recursion exactly like pnpm's
`resolvedDependencies`.

### Stage 1 — plumbing
Thread `wanted_lockfile: Option<Arc<Lockfile>>` from
`install_with_fresh_lockfile.rs` → `resolve_workspace` → `resolve_importer` →
`WorkspaceTreeCtx` (`resolve_dependency_tree.rs`). Also thread the active
`UpdateSeedPolicy` so the gate can suppress reuse for update-targeted names.
(Lands together with Stage 2 — an unused field would trip `-D warnings`.)

### Stage 2 — reuse gate (semver-satisfies)
Add a recursion parameter carrying the lockfile child-refs for the current subtree
(`Option<&BTreeMap<alias, resolved-ref>>`); at importer level it comes from
`lockfile.importers[id]` (`ProjectSnapshot.dependencies` + `.specifiers`).
In `resolve_node`, before the resolver call, compute a `reference`:
- importer dep: reuse only when the manifest specifier **semver-satisfies** the
  recorded version (`node-semver`), the dep isn't update-targeted, and the
  snapshot+package entry exist.
- transitive dep: take the ref from the passed-down child-refs map.
When matched, synthesize the `ResolveResult` from the lockfile (`PackageMetadata`
resolution + integrity; manifest reconstructed from the snapshot / read from the
store-index bundled manifest as the tarball-reuse path does) and skip the resolver.
Children still resolve normally in this stage.

### Stage 3 — subtree reuse (the real win)
When a node is reused and not update-propagated, build its children from the
snapshot's dep-refs (reuse `install_frozen_lockfile`'s walk) instead of
`extract_children` + recursion. Carry an `updated` flag down so an updated ancestor
discards the child-refs (passes `None`) and forces its subtree to re-resolve —
faithful to `parentPkg.updated ? undefined : refs`.

### Stage 4 — update suppression
Wire `UpdateSeedPolicy` (KeepAll / DropAll / DropOnly) into the gate so
`pacquet update [selector]` / `--latest` bypasses reuse for targeted deps and
propagates down their subtrees.

### Stage 5 — tests + benchmark
- Port pnpm's reuse/update suites (`resolveDependencies`, `install/update.ts`) as
  Rust tests first (per the "port tests before optimizations" rule).
- Discriminating no-re-resolve test: mockito server + dead-server, like #12096 — a
  second install with an unchanged dep must succeed with the registry down.
- Peer correctness: verify against the ported peer tests (pacquet's separate peer
  pass is the subtlest interaction).
- vlt.sh before/after on a deep-transitive fixture for the perf number.

## Risk
Stage 3 is high-blast-radius: wrong reuse → wrong tree → wrong installs. The peer
pass and the `updated`-propagation boundary are the subtlest parts.

## Known follow-ups (before un-drafting)

- ~~**Lockfile byte-ordering is build-order-dependent**~~ ([#12117](https://github.com/pnpm/pnpm/issues/12117)) — **fixed.**
  The writer now sorts every lockfile map by its rendered key at emit time
  (`serialize_yaml::sorted_map`), matching pnpm's `sortLockfileKeys`, so
  reuse and fresh resolves emit byte-identical lockfiles and a no-op
  re-install no longer reorders the file. The reuse-equivalence test now
  asserts byte-parity, and `reinstalling_an_unchanged_manifest_keeps_the_lockfile_byte_identical`
  guards re-install stability.
- **`overrides` drift** isn't yet guarded for transitive reuse (only
  `packageExtensions` is). An `overrides` change that rewrites a transitive
  dep's version should invalidate that subtree's reuse.
- **Dependency cycles conservatively re-resolve.** `subtree_fully_reusable` treats a
  still-in-progress back-edge as non-reusable, so any subtree containing a cycle is
  re-resolved rather than reused (correct, but a perf limitation). SCC-aware reuse of
  acyclic-equivalent cycles is a possible future optimization.
- vlt.sh before/after benchmark for the perf number.
