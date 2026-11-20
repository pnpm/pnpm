# Per-importer hoisting roots for `nodeLinker: hoisted` (multi-level output)

Close the last structural gap between pacquet's `@yarnpkg/nm` hoister port
(`pnpm/crates/real-hoist`) and upstream: per-importer hoisting roots. Today
`nm_hoist` runs `hoist_into_root` against the single virtual `.` root; upstream
`hoistTo` recurses into every node marked as a **hoist root** (workspaces under
`hoistingLimits: 'workspaces'`, direct deps under `'dependencies'`), producing a
multi-level tree where each bordered subtree gets its own fixed-point hoist.

## What works today (and must not regress)

- Non-root importers are attached to the tree unconditionally (v11 parity,
  pnpm/pnpm#12899), so cross-project version dedupe and conflict nesting work:
  a conflicting version stays nested at its position in the subtree and the
  walker (`hoisted_dep_graph.rs::walk_deps`) materializes it there.
- Hoisting **borders** are honored one level deep: a name in the root locator's
  `hoisting_limits` set blocks its descendants from hoisting past it
  (`real-hoist/src/lib.rs::hoist_subtree`, `under_border`).
- `hoist_workspace_packages` name-links (v11's `hoistedWorkspacePackages`) are
  a separate, implemented shape — they do not depend on this work.

## What multi-level adds

Within a bordered subtree (e.g. a workspace importer under
`hoistingLimits: 'workspaces'`), upstream still hoists *internally*: the
importer's transitive deps flatten up to the importer's own `node_modules`
instead of staying at their natural tree depth. Pacquet leaves them nested.
Nested placement is resolution-correct (Node walks up), so the user-visible
difference is layout/dedupe density, not resolvability — this is why the gap
has been shippable so far.

## Sketch

1. `get_hoisting_limits` already emits per-importer border entries for the
   `'dependencies'` mode ("become load-bearing once multi-level hoisting
   lands" — `package-manager/src/hoisting_limits.rs`). Keep that shape.
2. In `nm_hoist`, after the root fixed point, recurse: for each surviving
   child that is itself a hoist root (Workspace-kind, or name in a border
   set with a per-locator `hoisting_limits` entry), run `hoist_into_root`
   with that node as the root and its locator's border set. Upstream
   reference: `hoistTo` at yarnpkg-nm/sources/hoist.ts:329 and its
   `hoistIdents`/per-root preference maps.
3. The walker (`walk_deps`) needs no structural change — it already recurses
   into Workspace-kind children and places whatever the hoister decided.
4. Tests to port: upstream `hoist.test.ts` cases for `hoistingLimits`
   ('workspaces' and 'dependencies') and the CLI known-failure
   `partial_install_persists_hoisted_map` (pnpm/pacquet#433) which also
   depends on re-hoist merging.

## Risks / order of work

- The hoister's per-pass ident shift and preference maps are global today;
  per-root recursion needs them scoped per hoist root or recomputed per
  subtree (upstream recomputes: `buildPreferenceMap(rootNode)` per `hoistTo`).
- Do after pnpm/pacquet#433's partial-install work only if re-hoist merging
  is wanted in the same release; the passes are independent otherwise.
