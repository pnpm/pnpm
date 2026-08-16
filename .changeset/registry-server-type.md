---
"@pnpm/config.normalize-registries": minor
"@pnpm/lockfile.utils": minor
"@pnpm/resolving.tarball-url": minor
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

`serverType` tells pnpm how a registry lays out its tarball URLs. JFrog Artifactory repeats the scope in a scoped package's tarball filename (`@acme/widget/-/@acme/widget-1.0.0.tgz`) where the npm registry strips it (`@acme/widget/-/widget-1.0.0.tgz`). Declaring `serverType: artifactory` lets pnpm rebuild that URL, so it is omitted from `pnpm-lock.yaml` instead of being written out for every scoped package [pnpm/get-npm-tarball-url#16](https://github.com/pnpm/get-npm-tarball-url/issues/16).

The setting defaults to `npm` and is never inferred from the registry URL, so nothing changes unless you declare it. Because the lockfile depends on it, it belongs in `pnpm-workspace.yaml` rather than `.npmrc`; credentials are rejected there and still belong in `.npmrc`.
