---
"@pnpm/deps.graph-hasher": patch
"@pnpm/engine.runtime.system-node-version": minor
"pnpm": patch
---

**fix**: resolve the GVS hash's engine portion per-snapshot when a dependency declares its own `engines.runtime`, instead of using an install-wide value.

Pnpm's resolver desugars a dep's `engines.runtime` into `dependencies.node: 'runtime:<version>'`, and the bin linker spawns that dep's lifecycle scripts through the pinned Node downloaded into `<pkgDir>/node_modules/node/`. The GVS hash and the side-effects-cache key prefix were still anchored to the install-wide runtime — so a pinning snapshot's slot encoded the wrong Node major, and a reinstall on the same host could read the cached side-effects under a key whose `<platform>;<arch>;node<major>` triple disagreed with the Node the build actually ran on.

Per-snapshot resolution now matches what `bins/linker` already does on a per-package basis:

- `@pnpm/engine.runtime.system-node-version` adds `readSnapshotRuntimePin(children)` — reads the `node` entry from one snapshot's graph children and extracts the version from a `node@runtime:` value. Pairs with the existing `findRuntimeNodeVersion(snapshotKeys)` install-wide fallback.
- `@pnpm/deps.graph-hasher`'s `calcDepState` and `calcGraphNodeHash` consult `readSnapshotRuntimePin(graph[depPath].children)` first and only fall back to the install-wide `nodeVersion` parameter when the snapshot doesn't pin its own Node.

Pacquet mirrors the same precedence at the `calc_graph_node_hash` call site in `package-manager/src/virtual_store_layout.rs` — a new `find_own_runtime_node_major(snapshot)` helper reads each snapshot's `dependencies` for a `node` entry with `Prefix::Runtime` and overrides the install-wide engine when present.

On upgrade, snapshots of dependencies that declare their own `engines.runtime` re-hash under that dep's pinned Node instead of the install-wide value. The old slots become prune-eligible. Closes [#11690](https://github.com/pnpm/pnpm/issues/11690).
