## 1104.0.0

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

### Minor Changes

- A pnpr resolve request now carries the client's registries the way the `registries` setting declares them — keyed by URL, with the scopes routed to each, the bare-specifier prefix each answers to, and each one's `serverType` — in place of the prefix map it used to send.

  The server routes them through the same inversion the config reader runs, so a pnpr-served install resolves a scoped dependency from the registry that scope is routed to, which it previously could not: only the default registry and the prefix-addressed ones reached the server. A declared `serverType` reaches it too, so the tarball URLs pnpr omits from the lockfile match the ones the client reconstructs.

  Built-in scope routes the project has not pointed elsewhere are not declared, so a pnpr server's allowlist is not asked about `npm.jsr.io` on requests that resolve no JSR package.

  A registry a request only declares is no longer refused up front for being off the server's allowlist — a client describes its whole configuration, including scopes a given resolve never reaches, so a stray `@scope:registry` in a developer's `~/.npmrc` no longer fails every install against a pnpr server that does not serve it. The boundary moves to the fetch itself: an origin the resolve does reach is refused before the request leaves the server, with the same message.

  This changes the resolve and verify-lockfile request bodies. A pnpr server and its clients have to be on matching versions; the protocol is still experimental and unversioned.

- The `registries` setting now declares a registry once, keyed by its URL, with everything about that registry in the entry: how it lays out tarball URLs, the scopes routed to it, and the bare-specifier prefix it answers to.

  ```yaml
  registries:
    https://artifactory.example.com/artifactory/api/npm/npm-virtual/:
      serverType: artifactory
      scopes: ['@acme', '@acme-internal']
      prefix: work
  ```

  - **`serverType`** tells pnpm how the registry lays out its tarball URLs, which decides whether a URL can be omitted from `pnpm-lock.yaml`:
    - **undeclared** (the default) — strict. Only the exact canonical URL is treated as reconstructible.
    - **`npm`** — the registry behaves like `registry.npmjs.org`, which also serves a scoped package from its percent-encoded path. Declare this for a faithful mirror or caching proxy of the public registry so its tarball URLs can be omitted too.
    - **`artifactory`** — JFrog Artifactory repeats the scope in a scoped package's tarball filename (`@acme/widget/-/@acme/widget-1.0.0.tgz`) where the npm registry strips it (`@acme/widget/-/widget-1.0.0.tgz`). Declaring it lets pnpm rebuild that URL, so it is omitted from `pnpm-lock.yaml` instead of being written out for every scoped package [pnpm/get-npm-tarball-url#16](https://github.com/pnpm/get-npm-tarball-url/issues/16).
  - **`scopes`** lists the `@`-prefixed scopes that resolve from this registry. A bare `'@'` is the scope-less default registry, the one the `registry` setting names.
  - **`prefix`** is the alias a dependency addresses this registry by, as in `"foo": "work:^1.0.0"`.

  The layout is never inferred from the registry URL, so nothing changes unless you declare it; `registry.npmjs.org` continues to behave as `npm` without being declared. Because the lockfile depends on `serverType`, it is read from `pnpm-workspace.yaml` only — a `serverType` in the global `config.yaml` is ignored, so one developer's machine cannot shape a lockfile their collaborators read back with a different layout. Credentials are rejected in this setting, in a key as well as in a field, and still belong in `.npmrc`. An entry that routes nothing to itself and matches no configured registry is reported as a warning rather than silently ignored.

  ### Migrating

  The older `registries` shape, a map of `<scope>: <url>` strings, still works and needs no change:

  ```yaml
  registries:
    '@acme': https://npm.acme.example/
  ```

  `namedRegistries` is deprecated in favor of the `prefix` field, and is still read for prefixes `registries` does not declare.

  `toLockfileResolution` and `isCanonicalRegistryTarballUrl` now take their registry and layout as an options object rather than positional arguments, so `@pnpm/lockfile.utils` and `@pnpm/resolving.tarball-url` get a major bump.

- An install that had to re-hash store files to verify them now reports it. If that cost more than a second, it says how long — `The integrity of N files was checked in 2.5s.` — and if it was quick but covered more than a thousand files, it names the cause instead: their timestamps changed since the store recorded them, which a backup tool, an antivirus scan or a copied store can do.

### Patch Changes

- Kept pending build approvals available after removing an unrelated dependency.

- Fixed an issue where running `pnpm dedupe --check` in projects with `nodeLinker: hoisted` would cause dependencies to be moved out of `node_modules` into `node_modules/.ignored`.

- Fixed `pnpm install --merge-git-branch-lockfiles` deleting the per-branch lockfiles when the `lockfile` setting is `false`. Such an install never reads them, so it has nothing to merge them into and now leaves them alone.

- A resolve request now carries the client's `resolutionMode`, so an install delegated to a pnpr server picks versions the way the client would. `time-based` and `lowest-direct` reached the server as nothing at all, leaving it on its `highest` default: the returned lockfile pinned the highest satisfying version of every dependency, and the setting appeared to be ignored.

  This adds a field to the resolve request body. A server older than its client ignores it and keeps resolving `highest`; the protocol is still experimental and unversioned.

- Fixed `pnpm` installs using pnpr to honor the client's `autoInstallPeers`, `dedupePeers`, and `excludeLinksFromLockfile` settings [pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389).

- An override change is now absorbed by the fast lockfile update even when another, unchanged override uses the `catalog:` protocol. Previously any `catalog:`-valued override forced a full re-resolution whenever the override list changed, which could move unrelated packages in the lockfile (for example after `pnpm audit --fix` added an override).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.28
  - @pnpm/bins.remover@1100.0.21
  - @pnpm/building.after-install@1103.0.0
  - @pnpm/building.during-install@1102.0.17
  - @pnpm/building.policy@1100.0.20
  - @pnpm/catalogs.config@1100.0.6
  - @pnpm/config.normalize-registries@1101.0.0
  - @pnpm/config.parse-overrides@1100.1.4
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/constants@1102.0.0
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/deps.graph-hasher@1100.2.18
  - @pnpm/deps.path@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/exec.lifecycle@1100.1.14
  - @pnpm/fs.symlink-dependency@1100.0.18
  - @pnpm/hooks.read-package-hook@1100.2.5
  - @pnpm/hooks.types@1101.0.0
  - @pnpm/installing.context@1101.0.0
  - @pnpm/installing.deps-resolver@1102.0.0
  - @pnpm/installing.deps-restorer@1103.0.0
  - @pnpm/installing.linking.direct-dep-linker@1100.0.18
  - @pnpm/installing.linking.hoist@1100.0.28
  - @pnpm/installing.linking.modules-cleaner@1100.1.20
  - @pnpm/installing.modules-yaml@1101.0.0
  - @pnpm/installing.package-requester@1102.1.11
  - @pnpm/lockfile.filtering@1100.2.4
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.preferred-versions@1100.0.30
  - @pnpm/lockfile.pruner@1100.0.20
  - @pnpm/lockfile.settings-checker@1100.2.2
  - @pnpm/lockfile.to-pnp@1101.0.0
  - @pnpm/lockfile.utils@1102.0.0
  - @pnpm/lockfile.verification@1100.1.1
  - @pnpm/lockfile.walker@1100.0.20
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/patching.config@1100.1.2
  - @pnpm/pkg-manifest.utils@1100.4.1
  - @pnpm/pnpr.client@2.0.0
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/store.controller-types@1101.1.2
  - @pnpm/store.index@1100.2.5
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
