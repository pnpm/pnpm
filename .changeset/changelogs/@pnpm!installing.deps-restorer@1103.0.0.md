## 1103.0.0

### Major Changes

- `node_modules/.modules.yaml` no longer records the registries an install resolved from, and the recorded copy is dropped from the file on the first install that rewrites it.

  It dated from the lockfile format that spelled a dependency's path relative to its registry, where reading an installed tree meant knowing the registries it was installed with. Dependency paths have not carried a registry for several major versions, and the recorded copy outlived its use: `pnpm list`, `pnpm why`, and single-project installs preferred it over the project's own configuration, so a project whose registry had changed since its last install was still read through the old one.

  They now use the configured registries, like every other command already did.

- The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

  | before | after |
  |---|---|
  | `Config.registries` | `Config.registriesByScope` |
  | `Config.namedRegistries` | `Config.registriesByPrefix` |
  | `Config.registryOptions` | `Config.registryOptionsByUrl` |

  The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

  This is an internal rename: no setting, error code, lockfile field, or `.pnpmfile.cjs` hook field changes. A `preResolution` hook still reads `ctx.registries`, which is the name pacquet passes as well. The `registries` and `namedRegistries` settings are read under the names users write them.

  The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.

### Patch Changes

- Under `nodeLinker: hoisted`, a dependency declared against a peer-resolution variant of a package version is no longer dropped from the installed layout. All variants of a version share one hoisted copy, and edges pointing at any of them now resolve to it, so the depending project keeps the package in its `.package-map.json` and the depending package keeps it in its `node_modules/.bin`.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.28
  - @pnpm/building.during-install@1102.0.17
  - @pnpm/building.policy@1100.0.20
  - @pnpm/config.normalize-registries@1101.0.0
  - @pnpm/config.package-is-installable@1100.1.4
  - @pnpm/constants@1102.0.0
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/deps.graph-builder@1101.0.0
  - @pnpm/deps.graph-hasher@1100.2.18
  - @pnpm/deps.path@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/exec.lifecycle@1100.1.14
  - @pnpm/fs.symlink-dependency@1100.0.18
  - @pnpm/installing.linking.direct-dep-linker@1100.0.18
  - @pnpm/installing.linking.hoist@1100.0.28
  - @pnpm/installing.linking.modules-cleaner@1100.1.20
  - @pnpm/installing.linking.real-hoist@1100.1.14
  - @pnpm/installing.modules-yaml@1101.0.0
  - @pnpm/installing.package-requester@1102.1.11
  - @pnpm/lockfile.filtering@1100.2.4
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.to-pnp@1101.0.0
  - @pnpm/lockfile.utils@1102.0.0
  - @pnpm/patching.config@1100.1.2
  - @pnpm/pkg-manifest.reader@1100.0.17
  - @pnpm/store.controller-types@1101.1.2
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
