# Spec: `sideEffectsCache.engine` and `sideEffectsCache.key`

Source: [discussion 16039](https://github.com/orgs/pnpm/discussions/16039).

Status: proposed, not implemented.

## Objective

pnpm keys a cached build by the host that runs the install:

```text
<platform>;<arch>;node<major>;format=2[;deps=<hash>][;patch=<hash>]
```

A build script can target something else. VS Code installs three roots into one store, built for Electron, for the server's Node.js, and for the Node.js that runs the build. Each root passes its target to node-gyp through `updateConfig` `extraEnv`. All three compute the same key, so the server root restored the Electron build of `node-pty`. Projects like this turn the cache off and compile every native module on every fresh checkout.

The project knows what its builds depend on, and pnpm doesn't. With this feature the project gives pnpm two opaque strings. Each target gets its own cache entries and global virtual store slots in one store, and the cache can stay on.

pnpm v12 only. v11 gets bug fixes only, ignores both fields, and keeps today's keys.

## Configuration

```yaml title="pnpm-workspace.yaml"
sideEffectsCache:
  engine: darwin;arm64;electron43   # replaces <platform>;<arch>;node<major>
  key: <digest of the build inputs> # appended as ;key=<hash>
```

| Field | Effect on the key | Cost of a wrong value |
|---|---|---|
| `engine` | Replaces the host part, used as written. | Can merge entries that must stay apart. |
| `key` | Appends `;key=<hash>`. pnpm hashes the value, so paths in it never reach the store index or a server. | Can only split entries. Costs an extra build. |

- pnpm doesn't interpret either value.
- A project that sets `engine` owns the platform and architecture in it. A value that leaves them out shares builds between machines. The docs must say so.
- Both come from the same sources as `sideEffectsCache.exclude`: `pnpm-workspace.yaml`, the environment, and the result of an `updateConfig` hook. Hook output already round-trips through `WorkspaceSettings`, so no new hook API is needed.
- A value is a string or a map of package patterns, the patterns `exclude` uses. The first PR accepts the string form. A map fails with an error saying the per-package form isn't supported yet.

## Behavior

### Local cache key

```text
<engine>;format=2[;deps=<hash>][;patch=<hash>][;key=<hash>]
```

- `<engine>` is `sideEffectsCache.engine` when set. Otherwise pnpm resolves it as today, from the root project's `node@runtime:` pin, then `nodeVersion`, then `node --version`.
- `;key=` is present only when `key` is set.
- pnpm keeps owning `format`, `deps` and `patch`.
- With `engine` set, pnpm can key a build without a detected Node.js.

Code: [`calc_dep_state`](../crates/graph-hasher/src/dep_state.rs). The engine is resolved in [`resolve_engine_name`](../crates/deps-restorer/src/materialization_plan.rs), [`virtual_store_layout_for_lockfile`](../crates/deps-restorer/src/virtual_store_layout.rs) and [`package_map_engine_name`](../crates/package-manager/src/install/apply_materialization/selection.rs). All three must apply the override the same way.

### Global virtual store

- For a package in `build_required_dep_paths`, the slot hash uses the configured `engine` and adds a `key` field. That set holds every package that builds, plus every package that depends on one. Dependents belong in it because their slot links to the built package's slot. The engine already follows this rule.
- Every other slot hashes as today. Packages that need no build keep sharing slots across targets.

Code: [`calc_graph_node_hash`](../crates/graph-hasher/src/global_virtual_store_path.rs).

### Remote tier

- The remote input key leaves the engine out on purpose. When a field is set, pnpm appends `;engine=<hash>`, `;key=<hash>`, or both. An artifact built for one target is then never restored for another.
- The restore-time platform check still runs against the host.

Code: [`calc_dep_state_input_key`](../crates/graph-hasher/src/dep_state.rs).

### Commands

Every command that computes a cache key or a slot path sees the same `updateConfig` result as `pnpm install`.

- `pnpm rebuild` already runs the hooks.
- `pnpm store status` doesn't. The `store` dispatcher in [maintenance.rs](../crates/cli/src/cli_args/dispatch_query/maintenance.rs) loads config without `apply_update_config`, so it would look for the wrong slots. A hook-set `exclude` has the same gap today, and applying the hooks there fixes both. The `exclude` half is a bug in a released setting and gets its own changeset.
- Optimistic repeat installs record both fields, as they record `exclude` in [settings.rs](../crates/package-manager/src/install_state/settings.rs). A changed value forces a full install.
- pnpm's own download keeps the host engine. For `pnpm dlx`, see decision 6.

### Unset

With neither field set, every cache key, slot path and remote input key is byte-identical to today. Existing stores keep working.

## Security

A cloned repository controls `pnpm-workspace.yaml` and its pnpmfile, so it controls both values. The values only select cache entries and grant no trust. Signing material for the remote tier stays in global config, and the signature and platform checks still run on restore. Any project on the machine can already write builds under the host key, and `engine` doesn't change that.

## Out of scope

- The map form. It needs each package's `engine` and `key` folded into the `deps=` graph hash, which today covers a dependency's id but not its values.
- A package-declared field. It can later provide a default per-package value for projects that configure nothing.
- Hashing node-gyp's `npm_config_*` variables by default. Dropped in the discussion.
- The other ideas in the opening post: a message on cache reads and a flag that forces rebuilds.

## Testing

- `pnpm-graph-hasher` unit tests. Unset keys match today's expected strings. `engine` replaces the prefix and `key` appends one segment. The input key changes only when a field is set. A slot hash changes only for build-required packages.
- `pnpm-config` tests for parsing, reset, the resolved form, the environment, and the map-form error.
- An `updateConfig` test. A hook that returns `key` reaches `Config` and leaves `read` and `write` as they were.
- `pnpm-cli` tests with two projects sharing one store:
  - Different `key` or `engine` values make the second install run the build script. Equal values restore the build.
  - Under the global virtual store, the built package and its dependents get new slots, and the other slots stay where they were.
  - `pnpm rebuild` and `pnpm store status` agree with `pnpm install` on a hook-set value.
- Repeat-install drift for both fields.

Run `pnpm test:rust-affected`, or `pnpm test:rust -p pnpm-graph-hasher` for one crate.

## Docs and release

- Sections for `sideEffectsCache.engine` and `sideEffectsCache.key` in [build.md](../docs/settings/build.md), with entries in [settings.md](../docs/settings.md). Include an `updateConfig` example and the warning about platform and architecture in `engine`.
- A `pacquet` changeset with a minor bump.

## Success criteria

1. Two install roots that share a store and set different `key` values each build a native package once, then restore their own build on later installs.
2. With the same `engine`, builds made under two host Node.js versions share one cache entry.
3. With neither field set, no cache key, global virtual store slot or remote input key changes.
4. `pnpm install`, `pnpm rebuild` and `pnpm store status` compute the same keys and slots for a value set by `updateConfig`.
5. v11 behaves as before.

## Decisions to confirm

1. `read` and `write` in hook output. When the cache is off, a hook sees `sideEffectsCache: false`. The pattern `{ ...config.sideEffectsCache, key }` then returns `{ key }`. [`apply_side_effects_cache`](../crates/config/src/workspace_yaml/apply.rs) defaults a missing `read` and `write` to `true`, so the hook turns the cache back on, overriding `--no-side-effects-cache`. Proposal: in hook output, an omitted `read` or `write` keeps its current value.
2. `engine` and a package's own `node@runtime:` pin. The slot hash prefers a package's own pin over the install's engine ([graph_hash.rs](../crates/deps-restorer/src/virtual_store_layout/graph_hash.rs)). Proposal: a configured `engine` wins, since the project owns the value. The runtime dependency still changes `deps=`.
3. Which slots get `key`. The reply says "only of a built package". This spec reads that as the existing `build_required_dep_paths` set, dependents included. Gating on the built package alone would let two targets share a dependent's slot whose links point at different builds.
4. Empty strings. Proposal: reject them, so a hook bug can't silently fall back to shared keys.
5. Project-local `node_modules`. Without the global virtual store, should changing a value re-import built packages that are already linked, or is `pnpm rebuild` the answer?
6. `pnpm dlx`. It loads the project's `pnpm-workspace.yaml` without the hooks, so a value set there can reach the tool's install. Proposal: `dlx` ignores both fields, since it installs a tool for the host and not for the project's target.
