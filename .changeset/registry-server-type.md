---
"@pnpm/config.normalize-registries": minor
"@pnpm/lockfile.utils": major
"@pnpm/resolving.tarball-url": major
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pacquet": minor
"pnpm": minor
---

The `registries` setting now declares a registry once, keyed by its URL, with everything about that registry in the entry: how it lays out tarball URLs, the scopes routed to it, and the bare-specifier prefix it answers to.

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
