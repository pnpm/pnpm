# @pnpm/pnpr.client

Client library for the pnpr server. Resolves project dependencies server-side
and contains the TypeScript client for the shared side-effects artifact PoC.

## How it works

1. Sends `POST /-/pnpr/v0/resolve` to the pnpr server with the projects and the
   existing lockfile, when present.
2. The server resolves against the client's registries, verifies the input
   lockfile under the client's policy, and streams package records followed by
   the resolved lockfile.
3. Returns the lockfile for pnpm's headless install, which fetches the tarballs.
   See [pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230).

The resolver remains stateless unless its experimental shared-artifact feature
is enabled.

## Usage

This package is used internally by pnpm when the `pnprServer` config option is
set.

```typescript
import { resolveViaPnprServer } from '@pnpm/pnpr.client'

const { lockfile, stats } = await resolveViaPnprServer({
  registryUrl: 'http://localhost:4000',
  dependencies: { react: '^19.0.0' },
})

console.log(`Resolved ${stats.totalPackages} packages`)
// lockfile is ready for headless install
```

## Configuration

Add to `pnpm-workspace.yaml` to enable automatically during `pnpm install`:

```yaml
pnprServer: http://localhost:4000
```

## Shared side-effects artifact PoC

Set `resolver.artifacts: true` in pnpr's YAML to advertise and mount the
organization-scoped v0 endpoints. The feature is off by default and is not yet
wired into `pnpm install`. In this PoC, an `organization` owner's name must
equal the authenticated pnpr username; publisher-owned artifacts are rejected
until publisher discovery is defined.

The PoC implements the main trust boundary from the
[shared side-effects cache RFC](https://github.com/pnpm/rfcs/pull/20):

- `PUT /-/pnpr/v0/artifacts` stores an opaque signed envelope and its inline
  content-addressed blobs in the authenticated organization's namespace.
- `POST /-/pnpr/v0/artifacts/resolve` performs one batch lookup for candidate
  input keys and returns at most eight signed variants per key.
- `POST /-/pnpr/v0/artifacts/blob` reads one owner-scoped blob.
- `resolveSharedSideEffects` verifies the P-256 signature against an
  independently configured public key, validates the signed owner, input key,
  source integrity, manifest, and compatibility constraints, and picks the
  most preferred compatible variant.
- `downloadSharedArtifactBlob` recomputes SHA-512 before returning bytes.

The base64 `payload` in a `SignedArtifactEnvelope` contains the exact UTF-8 JSON
bytes covered by its DER-encoded `ecdsa-p256-sha256` signature. Signing those
opaque bytes avoids requiring JSON canonicalization across Rust and TypeScript
implementations. Input keys begin with `dependency-side-effects:v1:` and do not
contain host platform identity; compatibility tags live in the signed payload.

Publisher discovery, key distribution and revocation, lockfile pinning, and
automatic store import are deliberately left for a production protocol once
the RFC resolves those policy questions.
