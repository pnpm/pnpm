---
"@pnpm/config.reader": major
"@pnpm/exec.lifecycle": minor
"@pnpm/exec.prepare-package": minor
"@pnpm/fetching.git-fetcher": minor
"@pnpm/fetching.tarball-fetcher": minor
"@pnpm/fetching.binary-fetcher": minor
"@pnpm/installing.client": major
"@pnpm/config.commands": minor
"@pnpm/workspace.commands": minor
"@pnpm/engine.runtime.node-resolver": minor
"@pnpm/resolving.default-resolver": minor
"@pnpm/store.connection-manager": minor
"pnpm": minor
---

Renamed `rawConfig` to `authConfig` on the `Config` interface. This field now only contains auth/registry data from `.npmrc` files. Non-auth settings are no longer written to it.

Added `nodeDownloadMirrors` setting to configure custom Node.js download mirrors in `pnpm-workspace.yaml`:

```yaml
nodeDownloadMirrors:
  release: https://my-mirror.example.com/download/release/
  nightly: https://my-mirror.example.com/download/nightly/
```

Replaced `rawConfig: object` with `userAgent?: string` in lifecycle hook options. Removed unused `rawConfig` from fetcher and prepare-package options.
