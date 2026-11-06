---
id: ecosystems
title: Cargo and Python registries
---

Added in: v0.1.0-alpha.11

pnpr serves npm, Cargo, Python, and [container image](container-images.md)
registries from one instance. Every registry declares which ecosystem it serves,
and each ecosystem keeps its own protocol, its own routing, and its own default
registry.

## Addressing an ecosystem

Each ecosystem is served under its own path prefix, however many are
configured:

| Ecosystem | Prefix |
| --- | --- |
| npm | `/npm/` |
| Cargo | `/cargo/` |
| Python | `/pypi/` |
| Container images | `/oci/` |

`/<ecosystem>/~<name>/` addresses a named registry and `/<ecosystem>/` the
ecosystem's default. Adding an ecosystem to a server therefore changes no URL
an existing client uses.

Two things sit outside the prefixes. The default container image registry
answers at `/v2/` on the host root, because a client derives that path from the
image reference itself. And the endpoints that belong to no single ecosystem
sit on the root: `/-/ping`, the `/-/pnpr/v0/` protocols
(resolve, verify-lockfile, shared artifacts, pipeline runs, and the
cross-ecosystem publish), and the account endpoints that mint and manage tokens.

A `/~<name>/` prefix must arrive with a literal `~`. A percent-encoded `%7E`
does not select the named registry. `~` is unreserved, so a conforming client
sends it as written.

## Declaring registries

Give a hosted or upstream registry an `ecosystem:`; npm is the default. A router
may list sources of every ecosystem, because a request only ever sees the
sources that speak its own protocol, so one router can be the default target for
all of them.

```yaml title="pnpr.yaml"
registries:
  local:
    type: hosted
    packages:
      '@example/*': {}
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  crates:
    type: hosted
    ecosystem: cargo
    org: crates
    packages:
      my-crate: {}
  crates-io:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
  python:
    type: hosted
    ecosystem: pypi
    org: python
    packages:
      my-package: {}
  pypi-org:
    type: upstream
    ecosystem: pypi
    url: https://pypi.org/simple/
    public: true
  main:
    type: router
    sources: [local, npmjs, crates, crates-io, python, pypi-org]

defaultRegistry: main
```

For a `cargo` upstream, `url` is a sparse index root; for `pypi`, a Simple API
root.

### Reusing a name across ecosystems

Group the registries under an ecosystem key to give each protocol its own
registry of the same name:

```yaml title="pnpr.yaml"
registries:
  npm:
    internal:
      type: hosted
      packages:
        '@example/*': {}
    public:
      type: upstream
      url: https://registry.npmjs.org/
      public: true
    main:
      type: router
      sources: [internal, public]
  cargo:
    internal:
      type: hosted
    main:
      type: router
      sources: [internal]

defaultRegistry:
  npm: main
  cargo: main
```

`/npm/~internal` and `/cargo/~internal` are then independent registries. Router
sources resolve within their own group and cannot cross into another, and each
ecosystem gets its own default. Access rules, teams, upstream credentials, and
caches belong to the individual registry.

A grouped hosted registry defaults its `org` to its own `<name>`, and each
ecosystem stores its packages under its own `<ecosystem>/` directory in that
namespace. Set `org` explicitly to choose another namespace (`org: ''` for the
default namespace). Two hosted registries of the same ecosystem cannot share an
`org` namespace. The flat configuration format and its shared
`defaultRegistry: main` remain supported.

## Cargo

A Cargo registry serves a sparse index at `/cargo/index/` and the crates API at
`/cargo/api/v1/crates/`, or under `/cargo/~<name>/...` for a named registry.
Point `cargo` at it:

```toml title=".cargo/config.toml"
[registries.pnpr]
index = "sparse+https://pnpr.example.com/cargo/index/"
```

`cargo publish --registry pnpr`, `cargo yank --registry pnpr`, and dependencies
with `registry = "pnpr"` then go through pnpr. The `config.json` pnpr serves
points downloads back at itself, so proxied crates are cached and verified
against the upstream index checksum.

Use a pnpr token as the registry token. `cargo` sends it as a bare
`Authorization` header, and pnpr accepts that alongside `Bearer`. A registry
that is not anonymously readable advertises `auth-required`, so `cargo` sends
the token on index and download requests too.

Crate names are compared case-insensitively, and an exact `packages:` key must
be a valid crate name.

`cargo search` works against a pnpr registry, matching on crate name over the
crates it hosts. Each result reports the crate's newest release that is not
yanked; a crate whose releases are all yanked reports its newest release.

## Python

A Python registry serves the Simple Repository API at `/pypi/simple/` (PEP 503
HTML and PEP 691 JSON, chosen by `Accept`), files at
`/pypi/files/<project>/<filename>`, and accepts uploads on the legacy API at
`/pypi/legacy/`. A named registry answers under `/pypi/~<name>/...`.

```sh
pip install --index-url https://pnpr.example.com/pypi/simple/ my-package
twine upload --repository-url https://pnpr.example.com/pypi/legacy/ dist/*
```

Use `__token__` as the username and a pnpr token as the password, the PyPI
convention.

Project names are compared PEP 503 normalized, so `My_Package`, `my.package`,
and `my-package` are one project, and exact `packages:` keys are normalized the
same way. Proxied files are fetched from the URL the upstream page lists,
verified against its `sha256`, and cached. The project list at `/pypi/simple/`
enumerates hosted projects only.

Uploads must be valid wheel, `.tar.gz`, or `.zip` archives. The root
`METADATA` in a wheel's `.dist-info` directory or the root `PKG-INFO` in a
source distribution must name the same project and version as the filename
and upload form.

Publishers can yank a file with `PUT /pypi/files/<project>/<filename>/yank`
and a JSON body such as `{"reason":"Broken release"}`. A JSON body is required. An empty object yanks
without a reason. Reasons may contain up to 1024 UTF-8 bytes. `DELETE` on the same URL unyanks it. Yanked files remain
downloadable, and the Simple API reports their yank status to installers.

Administrators listed in `auth.admins` can remove a file with
`DELETE /pypi/files/<project>/<filename>`. This removes its Simple API entry
and makes downloads return 404. Stored archive bytes are retained. These routes
also work under a named registry's base URL. Deleted filenames cannot be uploaded again.

## Proxying crates.io and PyPI

Cargo and Python proxies restrict downloads and redirects to their configured
upstream origin and the existing [`routes.public`](configuration.md#routes)
allowlist. The official crates.io and PyPI upstreams also permit their download
hosts, `static.crates.io` and `files.pythonhosted.org`. A custom registry that
serves downloads from a separate host needs that host declared:

```yaml title="pnpr.yaml"
routes:
  public:
    - registry: https://downloads.example.com/
```

Configured headers are sent only over HTTPS or loopback HTTP, and a redirect
rebuilds its headers per destination, so configured credentials stay on the
upstream origin. Upstream credentials are never sent to the separate host an
index points downloads at.

## One publish for a workspace that spans ecosystems

`PUT /-/pnpr/v0/publish` (also mounted under `/npm/-/pnpr/v0/publish` and a
named npm registry's `/npm/~<registry>/-/pnpr/v0/publish` for
`pnpm publish --batch`) publishes packages of any ecosystem in a single
transaction. Under a named registry, its npm entries are published through that
registry. Each entry names its `ecosystem`, and an entry without one is an
npm publish document, so an npm-only batch body is already a valid one.

```json
{
  "packages": [
    { "name": "@acme/ui", "versions": {}, "_attachments": {} },
    { "ecosystem": "cargo", "metadata": { "name": "acme", "vers": "0.1.0" },
      "archive": "<base64 .crate>" },
    { "ecosystem": "pypi", "name": "acme", "version": "0.1.0",
      "filetype": "bdist_wheel", "filename": "acme-0.1.0-py3-none-any.whl",
      "content": "<base64 wheel>" }
  ]
}
```

Every entry is authorized and verified before any of them is written. Two
different failures follow from that, so it is worth keeping them apart.

**A batch that fails a check publishes nothing.** Authorization, validation, and
the version-conflict check all run up front, and any entry that fails one of them
aborts the whole batch before a byte is written.

**A batch that passes its checks is committed as one transaction.** It commits
as one generation of its org, so it lands whole or leaves nothing behind, and a
read sees the whole release or none of it:

- Losing a write race, to another writer publishing the same file between the
  check and the commit, fails the whole batch with `409`, and nothing is
  published. The bytes that won the slot are someone else's published release,
  and overwriting them would be the worse outcome. Republishing the batch after
  bumping the conflicting version completes the release.
- Every entry must belong to the same org. A batch that spans orgs is refused.
- A retry gets the original response back while none of its published npm versions
  have been unpublished. Retrying cannot restore an unpublished version. A client sends an
  `Idempotency-Key` header, or the digest of the whole request serves as the
  key. Reusing a key for a different request answers `409`. pnpr keeps the
  outcome for 7 days. A retry needs current publish permission like any
  publish. A caller whose permission was revoked since gets `403` and can
  read the outcome from the publication or the package.
- The commit is recorded as a publication. A top-level `"name": "v2.0.0"`
  field in the body names it. The name must be unique within the org, at most
  214 bytes long, and not a decimal number, which would read as a publication
  id.
  See [publication endpoints](endpoints.md#publication-endpoints).
- A top-level `"notes"` field is the publication's release note, and a
  `"notes"` field in an npm, Cargo, or Python entry is the note of the version
  it publishes. pnpr stores them with the release. A note breaking the
  [content rules](endpoints.md#release-note-endpoints) rejects the whole
  batch.

An [image manifest](container-images.md) whose layers are already uploaded can
ride along in the same batch. `GET /-/pnpr` advertises support as
`publish: [0]`.

### Staged archives {#staged-archives}

A publisher can upload each archive on its own before the publish that
references it, so one request body need not carry a whole release:

```text
PUT /-/pnpr/v0/artifacts/blob?integrity=sha512-…
```

The route is also mounted under `/npm/` and a named npm registry's
`/npm/~<registry>/`. A staged archive belongs to the account that uploaded it,
whichever of these addresses it went through.

`HEAD` on the same URL tells whether the archive is staged already, so a
resumed release uploads only what is missing. An upload is stored only if its
bytes match the integrity, and uploading stored bytes again is a no-op that
renews them.

`pnpm publish -r --batch` does this against a registry declared with
`serverType: pnpr`: it uploads every archive the registry does not hold yet,
two at a time, then sends a batch that references them. One commit references
at most 256 staged archives, so a batch of more packages carries its archives
inline.

A staged archive is not published and no registry route serves it. It belongs
to the account that uploaded it, and only a publish by that account can claim
it. It expires 24 hours after its last upload, and pnpr removes expired
archives at startup and every hour. One archive is at most 100 MiB, and one
account holds at most 256 archives and 2 GiB that no publish has claimed. An
upload reserves its size before its bytes arrive, so uploads running in
parallel count against those limits together, and an upload that stopped
gives its reservation back after an hour.

The bytes stay in the hosted store and their records in the
[metadata database](storage.md#sqlite-metadata). A sweep records an expired
archive as taken, in a transaction that holds only while no upload renewed it
since, before it removes the bytes. An upload of the same bytes afterwards
stores them anew under another key, so a renewal that races the sweep keeps
its archive on every storage backend.

An npm publish document, in a batch or on its own, references a staged archive
by putting its `integrity` in the attachment in place of `data`:

```json
"_attachments": {
  "ui-2.0.0.tgz": { "integrity": "sha512-…", "length": 4096 }
}
```

pnpr reads the archive while it stages the publish and verifies it against
the version's `dist` as it verifies an inline archive. Once a publish commits,
the archives it claimed stop counting against the account, and the next sweep
removes them. One publish names at most 256 staged archives and 2 GiB of
them, each reference counted. An archive the publishing account has not staged, or one
that has expired, fails the whole publish with `400`, and nothing of it is
published. A retry of a
committed request gets the recorded outcome without reading its archives
again, so it succeeds even after they are gone. A version's
[platform variants](endpoints.md#platform-variants) can be staged the same
way. A sigstore bundle and a publish
[held for approval](endpoints.md#staged-publishing-endpoints) carry their bytes
inline.

## JSR

`npm.jsr.io` is a built-in public route, like the npm registry, so JSR packages
resolve with no configuration. The built-in routes are reachable over HTTPS only.
