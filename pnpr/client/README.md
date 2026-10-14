# @pnpm/pnpr.client

Client library for the [pnpr](https://pnpm.io/pnpr) server. pnpm uses it when
the `pnprServer` setting is set. It covers two pnpr protocols:

- **Install acceleration.** Sends `POST /-/pnpr/v0/resolve` with the projects
  and the existing lockfile, and returns the resolved lockfile for pnpm's
  headless install, which fetches the tarballs itself. See
  [Install acceleration](https://pnpm.io/pnpr/install-acceleration).
- **Shared side-effects cache.** Looks up, verifies, restores, and publishes
  signed build artifacts through the `/-/pnpr/v0/artifacts` endpoints. See
  [Shared side-effects cache](https://pnpm.io/pnpr/shared-side-effects-cache).

## Usage

```typescript
import { resolveViaPnprServer } from '@pnpm/pnpr.client'

const { lockfile, stats } = await resolveViaPnprServer({
  registryUrl: 'http://localhost:7677',
  dependencies: { react: '^19.0.0' },
})

console.log(`Resolved ${stats.totalPackages} packages`)
// lockfile is ready for headless install
```

## Configuration

Add to `pnpm-workspace.yaml` to offload resolution during `pnpm install`:

```yaml
pnprServer: http://localhost:7677
```

The shared side-effects cache is configured under `sideEffectsCache.remote`.
The workspace names the organization and the eligible packages; the trust
material comes from the global config file or the environment:

```yaml
pnprServer: http://localhost:7677
sideEffectsCache:
  remote:
    org: acme
    packages:
      - native-addon
```

The server side of both protocols, including the `artifacts.orgs` access
rules, the signed-envelope format, and the platform compatibility tags, is
documented with pnpr.

## License

MIT
