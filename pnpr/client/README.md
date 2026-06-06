# @pnpm/pnpr.client

Client library for the pnpr server. Resolves a project's dependencies server-side and returns the resolved lockfile.

## How it works

1. Sends `POST /v1/install` to the pnpr server with the project's dependencies (and the existing lockfile, if any, for incremental resolution).
2. The server resolves against the client's registries, verifies the input lockfile under the client's policy, and answers with one gzipped JSON object carrying the resolved lockfile and stats.
3. Returns the resolved lockfile for use with pnpm's headless install, which fetches every tarball directly from the registries in parallel — like a normal install. See [pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230).

pnpr is a stateless resolver: it stores no tarballs and serves no file content.

## Usage

This package is used internally by pnpm when the `pnprServer` config option is set. It is not intended to be called directly, but can be used programmatically:

```typescript
import { fetchFromPnpmRegistry } from '@pnpm/pnpr.client'

const { lockfile, stats } = await fetchFromPnpmRegistry({
  registryUrl: 'http://localhost:4000',
  dependencies: { react: '^19.0.0' },
  devDependencies: { typescript: '^5.0.0' },
})

console.log(`Resolved ${stats.totalPackages} packages`)
// lockfile is ready for headless install
```

## Configuration

Add to `pnpm-workspace.yaml` to enable automatically during `pnpm install`:

```yaml
pnprServer: http://localhost:4000
```
