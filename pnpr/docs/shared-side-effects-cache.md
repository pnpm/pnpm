---
id: shared-side-effects-cache
title: Shared side-effects cache
---

Added in: pnpr v0.1.0-alpha.8, pnpm v11.25.0 and v12.0.0

:::caution Proof of concept

This is a proof of concept for
[pnpm/rfcs#20](https://github.com/pnpm/rfcs/pull/20). It is off on both sides
unless you turn it on, and its protocol may change without notice. It supports
Linux/glibc, macOS, and Windows on x64 and arm64. Lockfile pinning,
publisher-owned artifacts, and key lifecycle policy are deliberately left out.

:::

When a dependency has a build script, every machine that installs it pays for
that build. pnpm's local [`sideEffectsCache`](/settings/build#sideeffectscache)
makes the second install on the *same* machine cheap; this feature extends the
same idea across machines, by having one trusted builder publish the build
output and everyone else restore it.

The output is not trusted because it came from the server. It is trusted because
it is **signed**, with a P-256 key the consumer configured independently of the
server, and because the signed payload names exactly which package, which source
tarball, and which platform it belongs to.

## How an install uses it

1. pnpm plans which packages are eligible to build, before contacting pnpr.
2. It sends one batched `POST /-/pnpr/v0/artifacts/resolve` for the candidate
   input keys. The server returns at most eight signed variants per key.
3. For each candidate, pnpm verifies the ECDSA P-256 signature against a
   *locally configured* public key, then checks the signed package name and
   version, source tarball integrity, owner, input key, manifest, and platform
   compatibility, and picks the most preferred compatible variant.
4. It downloads that variant's blobs with `POST /-/pnpr/v0/artifacts/blob`, one
   request per unique SHA-512, recomputing the digest before accepting the
   bytes, and hydrates them into the store.
5. The restored side-effects map and its signed origin are persisted in the
   shared store, then selected before lifecycle scripts run, so the package's
   scripts are not executed.

On a later install, pnpm does not trust the persisted mapping merely because it
is local. It verifies the envelope again against the current public key and
checks its channel, owner, package and source identity, builder profile,
platform, manifest, policy, and stored files. An invalid remote envelope is
removed from consideration and quarantined for that pnpr server. The quarantine
is persisted in the store, so the same bad variant is not retried on every
install; another server remains an independent channel.

Any failure along that path — an unreachable server, an unverifiable signature,
an incompatible platform, a digest mismatch — falls back to the ordinary local
build. The feature can make an install faster; it can never make it fail.

## Enabling it on the server

The artifact surface is off by default and is independent of the resolver:

```yaml title="config.yaml"
artifacts:
  enabled: true
```

With `artifacts.enabled: false` (the default) the three routes are not mounted
at all, and the handshake at `GET /-/pnpr` does not advertise them. Since pnpr
v0.1.0-alpha.9, an artifact-only tier may set `resolver.enabled: false`, declare
no registries, and enable only `artifacts`; pnpm 12.1 can connect to that tier.

By default artifacts live at `<cache>/shared-artifacts/v0`. When the pnpr config
has an [`s3:` block](storage.md), they live under the reserved
`.pnpr-artifacts/v0/` namespace in that bucket instead. The S3 layout and
conditional quota updates let several stateless pnpr replicas share one
artifact tier.

Artifacts are stored per owner. In this proof of concept an `organization`
owner's name must equal the authenticated pnpr username, so the login name a
client uses is the organization it may read and write. Publisher-owned artifacts
are rejected until publisher discovery is defined.

pnpr enforces its own storage bounds: at most eight variants per input key,
1 GiB per owner, and 10 GiB across the server's artifact cache. Local storage
serializes updates with an advisory lock; S3 replicas coordinate the quota
counter with conditional object writes. A lookup's scanned envelope bytes plus
its serialized response share one 16 MiB budget.

## Configuring a consumer

The repository declares eligibility, and nothing else:

```yaml title="pnpm-workspace.yaml"
pnprServer: http://127.0.0.1:7677
allowBuilds:
  native-addon: true
sideEffectsCache:
  remote:
    org: acme
    packages:
      - native-addon
```

`packages` is an independent allowlist: a package is only a candidate when it is
listed there *and* has `requiresBuild: true`, *and* passes
[`allowBuilds`](/settings/build#allowbuilds), *and* has a verified source
integrity. `--ignore-scripts` returns without making a request at all.

The trust material — `trustedKeys`, `privateKey`, and everything else that
describes the act of signing — is refused in `pnpm-workspace.yaml` with
`ERR_PNPM_WORKSPACE_REMOTE_SIDE_EFFECTS_TRUST`. The repository being installed is
not a trust root: if it could name the signing key, a cloned repository could
turn the machine's key into a signing oracle. Those fields come from the [global
configuration file](/cli/config) instead, which travels with the machine:

```yaml title="~/.config/pnpm/config.yaml"
sideEffectsCache:
  remote:
    trustedKeys:
      acme-2026: '<base64 P-256 SubjectPublicKeyInfo DER public key>'
```

The repository and the machine each declare one half of the same section, and
they compose: neither file drops what the other set. `remoteSideEffectsCache` is
the older spelling of `sideEffectsCache.remote`, and `organization` of `org`;
both still work, so a machine configured before the rename keeps its trust
material.

Every field of the section is also settable from the environment, which wins
over both files — the shape a CI runner wants for material it must not commit:

| Environment variable | Setting |
| --- | --- |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_TRUSTED_KEYS` | `trustedKeys` (JSON object) |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_PRIVATE_KEY` | `privateKey` |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_PUBLISH` | `publish` |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID` | `keyId` |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_BUILDER_ID` | `builderId` |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_IMAGE_DIGEST` | `imageDigest` |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_ARCHITECTURE_BASELINE` | `architectureBaseline` |
| `PNPM_SIDE_EFFECTS_CACHE_REMOTE_BUILD_ENV` | `buildEnv` (JSON object) |

The `PNPM_REMOTE_SIDE_EFFECTS_CACHE_*` names still work, and the ones above win
when both are set.

The repository and the machine each contribute the half they own: a workspace
naming `org` and `packages` keeps whatever trust material the global
file or the environment supplied.

## Publishing from a builder

Publication is off unless a build explicitly turns it on, so only a trusted
builder uploads the diff its own build produced:

```sh
export PNPM_SIDE_EFFECTS_CACHE_REMOTE_PUBLISH=true
export PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID=acme-2026
export PNPM_SIDE_EFFECTS_CACHE_REMOTE_PRIVATE_KEY='<base64 P-256 PKCS#8 DER private key>'
export PNPM_SIDE_EFFECTS_CACHE_REMOTE_BUILDER_ID='ci/main/42'
```

`pnpm install` then runs the lifecycle scripts as usual, captures the actual
post-build diff, signs it, and stores it with
`PUT /-/pnpr/v0/artifacts`. `imageDigest`, `architectureBaseline` and `buildEnv`
are optional provenance recorded in the signed payload. Never commit the private
key.

A published artifact is immutable, the way a published `name@version` is. One
input key and one set of compatibility constraints admit one artifact, so
publishing a different build over an existing one answers `409 Conflict` while
republishing the identical one succeeds unchanged. A consumer that resolved an
artifact once is therefore never handed different bytes for it later, and no
publishing credential can replace one — releasing a claimed slot is an operator
action against the server's storage.

Immutability covers every consumer a variant can reach, not only an exactly
equal compatibility tag. pnpr rejects a new variant when its compatibility set
overlaps an existing variant for the same input key. A later `universal` build,
a broader build, or a higher-floor build therefore cannot take precedence for a
machine already served by an earlier artifact. Disjoint platform builds still
coexist: different operating systems, architectures, or Node.js majors have no
consumer in common.

Publication does not switch restoring off: a builder still looks the artifact up
first, and a hit skips the build the same way it does anywhere else — which
leaves nothing new to sign, since only an actual local build produces a diff to
publish. So a builder republishes exactly when it had to build, and to force one,
give it something the cache cannot answer (a package or platform with no stored
variant), rather than expecting `publish` to bypass the lookup.

Generate a key pair with Node.js:

<!-- cspell:disable -->
```sh
node -e "const {generateKeyPairSync}=require('node:crypto');const {privateKey,publicKey}=generateKeyPairSync('ec',{namedCurve:'prime256v1'});console.log('private='+privateKey.export({format:'der',type:'pkcs8'}).toString('base64'));console.log('public='+publicKey.export({format:'der',type:'spki'}).toString('base64'))"
```
<!-- cspell:enable -->

Keep the private key in the builder's environment, and hand the public key to
every machine that installs, under the same key id.

## Platform compatibility

An artifact says which platforms it is valid for, and the proof of concept
defines one narrow vocabulary rather than interpreting claims it does not
understand. `universal` is the positive claim for platform-independent output.
The tagged forms are:

```text
pnpm:v1:linux-<architecture>-node<major>-glibc<major>.<minor>
pnpm:v1:darwin-<architecture>-node<major>-macos<major>.<minor>
pnpm:v1:win32-<architecture>-node<major>-windows<major>.<minor>.<build>
```

`architecture` is `x64` or `arm64`, and every numeric component is a canonical
unsigned decimal. A consumer generates the tags for its own glibc version down
to minor zero, most recent floor first — glibc 2.3 advertises `glibc2.3`,
`glibc2.2`, `glibc2.1`, `glibc2.0` — and matching is exact against that ordered
set, so an artifact built against a 2.1 floor serves a 2.3 consumer.

A macOS consumer advertises its product-version major and minor, and a Windows
consumer advertises its NT kernel major, minor, and build. For both, the
operating system, architecture, and Node.js major must match exactly, while the
consumer's OS version must be at least the artifact's declared floor. The
greatest compatible floor wins. A tagged match beats `universal`, and equal-rank
variants are ordered by ascending signed-envelope digest.

An unknown schema, platform, or dimension, and any malformed tag, is a miss. No
other platform or libc family is treated as compatible by guessing.

## Signed envelopes

The base64 `payload` of a signed envelope is the exact UTF-8 JSON covered by the
`ecdsa-p256-sha256` signature, and verification always uses those unchanged
bytes. Signing opaque bytes avoids having to agree on JSON canonicalization
between the Rust and TypeScript implementations. Signatures are canonical ASN.1
DER; verification keys are P-256 `SubjectPublicKeyInfo` DER; `keyId` is an
opaque case-sensitive UTF-8 string of 1–256 bytes with no control characters.

The envelope's own digest is SHA-256 over these fields, in this order, regardless
of how the outer JSON object was ordered:

```text
pnpm-shared-artifact-envelope-v1\0
algorithm\0
keyId\0
decoded payload\0
decoded DER signature
```

Since pnpr v0.1.0-alpha.9, every candidate and signed payload carries a
discriminated subject. Dependency side effects use
`{ kind: 'dependency-side-effects', package, sourceIntegrity }` and an input key
beginning with `dependency-side-effects:v1:`. The protocol also defines
`{ kind: 'workspace-task', project, task }` with a `workspace-task:v1:` input
key, so workspace-task artifacts cannot be confused with dependency builds.
The current pnpm integration publishes and restores dependency side effects;
the workspace-task subject reserves the protocol identity for task artifacts.

Input keys carry no host platform identity; compatibility tags live in the
signed payload instead. The artifact kind and input-key prefix must match the
subject, and its input key, subject, and owner must match the candidate. For a
dependency, that binds the signed package name and version and source tarball
integrity to the package being installed. Organization eligibility is supplied
independently by the caller and checked before lookup.
