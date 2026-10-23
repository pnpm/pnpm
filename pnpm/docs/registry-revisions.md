---
id: registry-revisions
title: Registry Revisions
---

Added in: v12.0.0

Some registries can serve a **replacement artifact** for a version that is
already published (a rebuild with a vulnerability patched out, for example)
without changing the version number and without rewriting the bytes the original
`name@version` URL has always served. pnpm calls each such artifact a *revision*
of that version.

Revision 0 is always the original. A registry that replaces an artifact
advertises the replacement as revision 1, the one after that as revision 2, and
so on, per registry and per `name@version`.

Revisions are experimental. Their metadata and lockfile format may change
without a major release.

:::note

Revisions are a registry capability, not something pnpm can add to a registry
that does not implement them. [pnpr](/pnpr) serves them, both for the packages it
hosts and, as a proxy, for an upstream registry that advertises them.
`registry.npmjs.org` does not.

:::

## How a revision is addressed

A revision is fetched by its own digest, from an immutable route on the
registry:

```text
<registry-base>/-/tarballs/sha512/<base64url-digest>
```

The digest is the hash of the revision's uncompressed tar, on every registry
and for every revision, the original included. The registry may compress the
response with `gzip` or `zstd`. pnpm removes that compression and checks the
tar, so the URL and its integrity stay the same whatever the registry sends.

Registry metadata advertises that URL together with an ordinary Subresource
Integrity value and the revision number:

```jsonc
{
  "name": "ejs",
  "version": "2.7.4",
  "dist": {
    "tarball": "https://registry.example/-/tarballs/sha512/AbCd...",
    "integrity": "sha512-AbCd...",
    "revision": 2
  }
}
```

A revision-aware version always carries `dist.revision`, including `0` when the
original is selected. `dist.revisions` carries the history: every advertised
revision with its own integrity, digest URL, and manifest. It always holds
revision 0. A version without either field is an ordinary version outside the
revision protocol.

pnpm validates all of that during resolution:

* the URL must sit on the selected registry's integrity-tarball route, and its
  base64url digest must decode to exactly the digest in `dist.integrity`;
* the revision number must be an integer in `0 ≤ N ≤ 2^53 − 1`;
* the history must hold revision 0 and the selected revision.

Anything else fails with `ERR_PNPM_MALFORMED_METADATA`, and so does a
digest-route URL without `dist.revision`. The fetch itself is a single
authenticated request with no retry and no fallback, and **every** redirect,
same-origin included, fails it.

## In the lockfile

Every revision is recorded with its revision number, the original included, and
the hash of its tar:

```yaml title="pnpm-lock.yaml"
packages:
  ejs@2.7.4:
    resolution:
      integrity: sha512-<original-tar-digest>
      revision: 0

  lodash@4.17.21:
    resolution:
      integrity: sha512-<replacement-tar-digest>
      revision: 1
```

The `revision` line is what tells pnpm to fetch the tar by digest instead of
reconstructing the canonical URL. It also makes the diff readable: a change from
`revision: 1` to `revision: 2` beside a changed integrity says *why* the
integrity changed. An entry without `revision` is an ordinary entry, fetched
from the canonical URL as always.

A lockfile written by an earlier pnpm v12 version records an original selected
through the revision protocol as an ordinary entry. Such an entry stays valid
for the canonical URL, and the next resolution records it as `revision: 0`.

An installed lockfile is never silently upgraded: a fetch keeps the locked
revision rather than adopting whatever the registry currently selects.

## Selecting a revision

A specifier may pin a revision with `+rN`, carried as semver build metadata:

```yaml title="pnpm-workspace.yaml"
overrides:
  ejs@2.7.4: 2.7.4+r1
```

* `+r0` keeps the original: the opt-out from a replacement the registry has
  selected. It is recorded as `revision: 0`.
* A positive number adopts a replacement the registry advertises but has not
  selected, or freezes one it has.

It works as an override target and as a directly declared dependency, across
npm, JSR, aliases, and [named registries](./registries.md#prefix). The version
half resolves normally (build metadata is ignored for version matching), and
then the named revision is selected out of `dist.revisions`, with **that
entry's own manifest** driving subtree resolution. Revisions may legitimately
differ in their dependencies, peer dependencies, `bin`, `engines`, and
install-script posture, so reading the current version's top-level fields would
be wrong.

The rules around it:

* A revision the registry does not advertise is a hard error
  (`ERR_PNPM_NO_MATCHING_REVISION`); pnpm never falls forward to the selected
  one.
* On a version that is not revision-aware, `+r0` is satisfied by the
  only artifact, which stays an ordinary entry, and a positive number is an
  error.
* Version-changing targets compose: `"ejs@2.7.4": "2.7.5+r1"` rewrites the spec
  to `2.7.5` and then selects its revision 1.
* Rewriting applies once, so revision selection can neither chain nor cycle.
* Only the `r<digits>` build-metadata namespace is reserved. A spec carrying any
  other build metadata is an ordinary spec.
* Two specs demanding different revisions of one `name@version` in the same
  graph fail with `ERR_PNPM_REVISION_CONFLICT`. One package key can hold only one
  artifact, and pnpm will not silently unify them.

Like every exact-selector override, a `+rN` override **pins the version** too: a
dependency declared `^2.7.0` stays on `2.7.4` after upstream publishes `2.7.5`,
until the override is changed or removed.

Selection is a preference, not a security boundary. A registry whose policy
refuses a revision's bytes answers its digest URL with a `403`, and the pinned
install then fails loudly, naming the policy and the advertised revisions.

## Refreshing revisions

To move to the artifacts a registry currently selects, without changing any
package version:

```sh
pnpm update --patches
```

For every locked registry package, pnpm resolves current metadata for the same
exact `name@version`. When the selected artifact changed, it updates the
integrity, the `revision` field, and the whole package snapshot together.
Dependency metadata may differ between revisions, so changing only the checksum
would be wrong.

A package whose revision is pinned explicitly is skipped, and the pin holds: that
covers an override with a `+rN` target and a dependency declared as one, so
`"ejs": "2.7.4+r1"` stays on revision 1 across every refresh.

`--patches` cannot be combined with package selectors, `--latest`,
`--interactive`, or `--global` (`ERR_PNPM_PATCHES_WITH_SELECTOR`).

The refresh can be resolved server-side by a pnpr server, either from
[`pnprServer`](/pnpr/install-acceleration) or from `--pnpr-server <url>` on the
command line. An explicit refresh bypasses frozen reuse and pnpr's
whole-resolution cache, and the registry resolver revalidates metadata even when
its caches are warm. Otherwise the command could hand back exactly the
revisions it was asked to look past.
