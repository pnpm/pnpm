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
organization-scoped v0 endpoints. The feature is off by default. Both the
TypeScript and Rust CLIs automatically query it during normal and frozen
lockfile installs when `pnprServer` and `sharedSideEffectsCache` are configured.
In this PoC, an `organization` owner's name must equal the authenticated pnpr
username; publisher-owned artifacts are rejected until publisher discovery is
defined.

Add the client policy to `pnpm-workspace.yaml`:

```yaml
pnprServer: http://127.0.0.1:7677
allowBuilds:
  native-addon: true
sharedSideEffectsCache:
  organization: acme
  packages:
    - native-addon
```

`packages` is an independent eligibility allowlist. A package must also have
`requiresBuild: true`, pass `allowBuilds`, and have a verified source integrity.
Signing keys are deliberately not accepted from `pnpm-workspace.yaml`, because
the repository being installed is not a trust root. Set the user-controlled
`PNPM_SHARED_SIDE_EFFECTS_CACHE_TRUSTED_KEYS` environment variable to a JSON
object mapping key IDs to base64 P-256 public keys in DER encoding.
`--ignore-scripts` disables remote reuse. An unavailable server, invalid
signature, incompatible platform, or bad blob falls back to the ordinary local
build. The PoC supports Linux glibc on x64 and arm64.

Set these environment variables only on a trusted builder to publish the build
diff produced by `pnpm install`:

```sh
export PNPM_SHARED_SIDE_EFFECTS_CACHE_PUBLISH=true
export PNPM_SHARED_SIDE_EFFECTS_CACHE_KEY_ID=acme-2026
export PNPM_SHARED_SIDE_EFFECTS_CACHE_PRIVATE_KEY='<base64 P-256 PKCS#8 DER private key>'
export PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILDER_ID='ci/main/42'
```

Optional provenance fields are
`PNPM_SHARED_SIDE_EFFECTS_CACHE_IMAGE_DIGEST`,
`PNPM_SHARED_SIDE_EFFECTS_CACHE_ARCHITECTURE_BASELINE`, and
`PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILD_ENV` (a JSON object whose values are
strings). Do not commit the private key.

### Local trial

Build this branch first:

```sh
pnpm install
pnpm --filter pnpm run compile
cargo build -p pnpr
```

Start pnpr with a temporary config that enables account creation and artifacts:

```yaml
storage: /tmp/pnpr-shared-artifacts/storage
cache: /tmp/pnpr-shared-artifacts/cache
secret: replace-with-a-local-secret-at-least-32-bytes
resolver:
  enabled: true
  artifacts: true
auth:
  htpasswd:
    file: /tmp/pnpr-shared-artifacts/htpasswd
    max_users: 1
```

```sh
target/debug/pnpr --config /tmp/pnpr-shared-artifacts/config.yaml
node pnpm11/pnpm/dist/pnpm.mjs login --registry=http://127.0.0.1:7677
```

Use the login name as `sharedSideEffectsCache.organization`. The login writes
the bearer token that pnpm reuses for artifact publication, lookup, and blob
downloads. Generate a P-256 key pair with Node.js:

<!-- cspell:disable -->
```sh
node -e "const {generateKeyPairSync}=require('node:crypto');const {privateKey,publicKey}=generateKeyPairSync('ec',{namedCurve:'prime256v1'});console.log('private='+privateKey.export({format:'der',type:'pkcs8'}).toString('base64'));console.log('public='+publicKey.export({format:'der',type:'spki'}).toString('base64'))"
```
<!-- cspell:enable -->

Keep the printed private key in the trusted builder environment. Put the public
key in the user environment that runs installs:

```sh
export PNPM_SHARED_SIDE_EFFECTS_CACHE_TRUSTED_KEYS='{"acme-2026":"<printed public key>"}'
```

Run the first install with the publication variables set. Then unset
`PNPM_SHARED_SIDE_EFFECTS_CACHE_PUBLISH`, remove the project's `node_modules`,
and run the same install again. Keep both `sideEffectsCache` and
`sideEffectsCacheReadonly` false while testing if the same machine's ordinary
local side-effects cache would otherwise hide the remote lookup. pnpr should log
one batch resolve and blob reads, and the second install should materialize the
built files without running the package's lifecycle scripts. Use
`just cli -- install` instead of the bundled JavaScript CLI to exercise the Rust
implementation.

The PoC implements the main trust boundary from the
[shared side-effects cache RFC](https://github.com/pnpm/rfcs/pull/20):

- `PUT /-/pnpr/v0/artifacts` stores an opaque signed envelope and its inline
  content-addressed blobs in the authenticated organization's namespace.
- `POST /-/pnpr/v0/artifacts/resolve` performs one batch lookup for candidate
  input keys and returns at most eight signed variants per key. Envelope bytes
  scanned plus serialized response bytes share one 16 MiB lookup budget.
- `POST /-/pnpr/v0/artifacts/blob` reads one owner-scoped blob.
- `resolveSharedSideEffects` verifies the P-256 signature against an
  independently configured public key, validates the signed package identity,
  owner, input key, source integrity, manifest, and compatibility constraints,
  and picks the most preferred compatible variant. It only submits candidates
  that passed both package eligibility and `allowBuild`, and returns without a
  request when `--ignore-scripts` is effective.
- `downloadSharedArtifactBlob` recomputes SHA-512 before returning bytes.

Publication serializes the variant-count check and envelope write with a
cross-process filesystem lock, so pnpr processes sharing one local cache enforce
the eight-variant cap together. The PoC rejects writes above 1 GiB per owner or
10 GiB across the server's artifact cache.

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
digest mismatch rejects the selected variant for the current install before its
mapping reaches the importer. The PoC exposes the verified envelope digest and
the digest-verifying download primitive; persistent quarantine is deferred to
the production protocol.

Publisher discovery, key distribution and revocation, lockfile pinning, and
persistent quarantine are deliberately left for a production protocol once the
RFC resolves those policy questions. Remote mappings are kept in memory for the
current install and are looked up and revalidated again on later installs; they
are not persisted as unlabelled local side-effects entries.
