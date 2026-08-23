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

- `pnpm config get` and `pnpm config list` now show the settings pnpm acts on under their documented names:

  - `registries` shows the registries pnpm resolves from, merged across every source (`.npmrc`, `pnpm-workspace.yaml`, the global config, CLI flags), in the shape the setting is written in: keyed by registry URL, with the default registry declared as the bare `@` scope. Built-in routes are included — the `@jsr` scope and the `npmjs` and `gh` prefixes — unless pointed elsewhere. Previously `pnpm config get registries` printed `undefined`.
  - `update` and `audit` show the effective sections, whichever spelling set them. The deprecated internal spellings (`updateConfig`, `auditConfig`, `auditLevel`) are no longer listed.
  - `catalogs` shows the complete resolved catalog set — the singular `catalog` block is its `default` entry — whichever spelling declared it.
  - The `registry` and `@scope:registry` entries show the merged routes rather than raw `.npmrc` values, so they always agree with the `registries` view.

- A registry can now declare that its abbreviated metadata carries the `time` field, so `resolutionMode: time-based` reads the full metadata document only from the registries that need it:

  ```yaml
  resolutionMode: time-based
  registries:
    https://npm.internal.example/:
      supportsTimeField: true
  ```

  `registry.npmjs.org` omits `time` from abbreviated metadata, so a time-based resolution has to fall back to the much larger full document. That fallback used to be all-or-nothing: `registrySupportsTimeField` answered for every registry at once, so a project resolving from both the public registry and a Verdaccio instance either paid for full metadata everywhere or claimed a `time` field npmjs does not serve. The answer is now per registry, and `registrySupportsTimeField` remains the answer for every registry that does not declare one.

  The declaration is also sent to a pnpr server, which applies it to the resolution it runs on the client's behalf.

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

### Patch Changes

- Updated dependencies:
  - @pnpm/constants@1102.0.0
  - @pnpm/types@1102.0.0
