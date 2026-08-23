## 1101.0.0

### Major Changes

- The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

  | before | after |
  |---|---|
  | `Config.registries` | `Config.registriesByScope` |
  | `Config.namedRegistries` | `Config.registriesByPrefix` |
  | `Config.registryOptions` | `Config.registryOptionsByUrl` |

  The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

  This is an internal rename: no setting, error code, lockfile field, or `.pnpmfile.cjs` hook field changes. A `preResolution` hook still reads `ctx.registries`, which is the name pacquet passes as well. The `registries` and `namedRegistries` settings are read under the names users write them.

  The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.

### Minor Changes

- When `enableGlobalVirtualStore` is on, every process pnpm spawns for the project (`pnpm run`, `pnpm exec`, lifecycle scripts) now receives a `NODE_PATH` pointing at the project's hoisted `node_modules`, plus a `NODE_OPTIONS` `--import` flag that registers a resolve hook restoring `NODE_PATH` lookups for ESM imports. Dependencies that import undeclared ("phantom") packages keep resolving under the global virtual store — for both CommonJS and ESM — without installing the `@pnpm/plugin-esm-node-path` config dependency [pnpm/pnpm#9618](https://github.com/pnpm/pnpm/issues/9618). Tools run by `pnpm dlx` resolve such dependencies too: the JS CLI passes them the same environment, while the Rust CLI's dlx cache is self-contained, so its layout already exposes them.

### Patch Changes

- `pnpm exec --recursive --no-reporter-hide-prefix` no longer prints a blank prefixed line after each chunk of a command's output, and no longer splits a line in two when it straddles a chunk boundary.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.15
  - @pnpm/building.commands@1101.0.0
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.reader@1102.0.0
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/deps.status@1100.1.18
  - @pnpm/engine.runtime.commands@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/exec.lifecycle@1100.1.14
  - @pnpm/installing.client@1100.3.5
  - @pnpm/installing.commands@1101.0.0
  - @pnpm/pkg-manifest.reader@1100.0.17
  - @pnpm/store.path@1100.0.6
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.injected-deps-syncer@1100.0.34
  - @pnpm/workspace.project-manifest-reader@1100.0.25
  - @pnpm/workspace.projects-sorter@1100.0.15
