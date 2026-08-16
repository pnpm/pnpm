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

Added a `registryOptions` setting for declaring per-registry facts that pnpm cannot infer, keyed by registry URL:

```yaml
registryOptions:
  https://artifactory.example.com/artifactory/api/npm/npm-virtual/:
    serverType: artifactory
```

`serverType` tells pnpm how a registry lays out its tarball URLs, and has three states:

- **undeclared** (the default) — strict. Only the exact canonical URL is treated as reconstructible.
- **`npm`** — the registry behaves like `registry.npmjs.org`, which also serves a scoped package from its percent-encoded path. Declare this for a faithful mirror or caching proxy of the public registry so its tarball URLs can be omitted too.
- **`artifactory`** — JFrog Artifactory repeats the scope in a scoped package's tarball filename (`@acme/widget/-/@acme/widget-1.0.0.tgz`) where the npm registry strips it (`@acme/widget/-/widget-1.0.0.tgz`). Declaring it lets pnpm rebuild that URL, so it is omitted from `pnpm-lock.yaml` instead of being written out for every scoped package [pnpm/get-npm-tarball-url#16](https://github.com/pnpm/get-npm-tarball-url/issues/16).

The layout is never inferred from the registry URL, so nothing changes unless you declare it; `registry.npmjs.org` continues to behave as `npm` without being declared. Because the lockfile depends on this setting, it belongs in `pnpm-workspace.yaml` rather than `.npmrc`; credentials are rejected there and still belong in `.npmrc`. An entry whose key matches no configured registry is reported as a warning rather than silently ignored.

`toLockfileResolution` and `isCanonicalRegistryTarballUrl` now take their registry and layout as an options object rather than positional arguments, so `@pnpm/lockfile.utils` and `@pnpm/resolving.tarball-url` get a major bump.
