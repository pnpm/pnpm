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
  independently configured public key, validates the signed package identity,
  owner, input key, source integrity, manifest, and compatibility constraints,
  and picks the most preferred compatible variant. It only submits candidates
  that passed both package eligibility and `allowBuild`, and returns without a
  request when `--ignore-scripts` is effective.
- `downloadSharedArtifactBlob` recomputes SHA-512 before returning bytes.

### v0 compatibility tags

The PoC intentionally defines one narrow tagged platform vocabulary rather
than interpreting unknown producer claims. `universal` is the positive claim
for platform-independent output. The only `tagged` form is:

```text
pnpm:v1:linux-<architecture>-node<major>-glibc<major>.<minor>
```

`architecture` is `x64` or `arm64`; all numeric components are canonical
unsigned decimals without leading zeroes. A consumer generates tags for its
glibc version down to minor zero, most recent floor first. For example, glibc
2.3 advertises `glibc2.3`, `glibc2.2`, `glibc2.1`, and `glibc2.0`. Matching is
exact against that ordered set, so an artifact tagged with a 2.1 floor serves a
2.3 consumer. Tagged matches beat `universal`; equal-rank variants are ordered
by ascending signed-envelope digest. Unknown schemas, platforms, dimensions,
or malformed tags are misses. Other platforms and libc families remain out of
the PoC instead of being treated as compatible guesses.

`platformFingerprint` is SHA-256 over the ASCII bytes
`pnpm-platform-fingerprint-v1\0`, followed by every canonical supported tag in
preference order and a NUL byte after each tag. Duplicate tags and lists longer
than 64 entries are rejected.

### Signed envelope and blobs

The base64 `payload` in a `SignedArtifactEnvelope` contains the exact UTF-8 JSON
bytes covered by an `ecdsa-p256-sha256` signature. Payload and signature use
canonical padded base64; the signature uses canonical ASN.1 DER. Signing those
opaque bytes avoids requiring JSON canonicalization across Rust and TypeScript
implementations: JSON property order and manifest serialization are whatever
the signer emitted, and verification always uses those unchanged decoded
bytes. `keyId` is an opaque, case-sensitive UTF-8 string of 1–256 bytes without
control characters. Verification keys are P-256 SubjectPublicKeyInfo DER. The
outer envelope object's JSON property order is irrelevant; its digest is
SHA-256 over these fields and decoded values in this fixed order:

```text
pnpm-shared-artifact-envelope-v1\0
algorithm\0
keyId\0
decoded payload\0
decoded DER signature
```

Input keys begin with `dependency-side-effects:v1:` and do not contain host
platform identity; compatibility tags live in the signed payload. The signed
package name and version, source tarball integrity, and owner must all match the
current candidate. A publisher owner must additionally equal the signed package
name. Organization eligibility is supplied independently by the caller and is
checked before lookup.

Blob reads use the same authorization as lookup and send one owner-scoped
`POST /-/pnpr/v0/artifacts/blob` request per unique SHA-512 integrity. Callers
may issue those independent requests in parallel. A non-success response or a
digest mismatch is a cache miss; a mismatch must quarantine the selected
envelope digest before any bytes reach CAFS or the importer. The PoC exposes the
verified envelope digest and the digest-verifying download primitive; persistent
quarantine is part of the deferred automatic store integration.

Publisher discovery, key distribution and revocation, lockfile pinning,
persistent quarantine, and automatic store import are deliberately left for a
production protocol once the RFC resolves those policy questions.
