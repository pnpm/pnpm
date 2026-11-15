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
all of them. Store the registries with
[`pnpr registries import`](cli.md#registries):

```yaml title="registries.yaml"
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
    namespace: crates
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
    namespace: python
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

### Defaults for each ecosystem

Registry names are unique across the stored registries. Declare every registry
directly under `registries:` and select its protocol with `ecosystem:`.
Nesting registries under ecosystem keys is rejected.

`defaultRegistry` can name a shared router, as above, or select a target for
each ecosystem:

```yaml title="registries.yaml"
defaultRegistry:
  npm: local
  cargo: crates
  pypi: python
```

Each target must serve the selected ecosystem. A default does not change a
registry's ecosystem or storage namespace.

Hosted registries use the [default storage namespace](storage.md#layout) when
`namespace` is omitted. Set `namespace` to choose another namespace. Two hosted
registries of the same ecosystem cannot share a `namespace`.

## Cargo

A Cargo registry serves a sparse index at `/cargo/index/` and the crates API at
`/cargo/api/v1/crates/`, or under `/cargo/~<name>/...` for a named registry.
Hosted and proxied crate index files send a strong `ETag`. Requests with a
matching `If-None-Match` receive `304 Not Modified` with no body.

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

Crate names are compared case-insensitively, with `_` and `-` treated as
equivalent. An exact `packages:` claim covers both spellings, including when
the crate has not been published yet. Keys must be valid crate names.
Use the first published spelling in Cargo dependency declarations and when
publishing new versions.

`cargo search` works against a pnpr registry, matching on crate name over the
crates it hosts. Each result reports the crate's newest release that is not
yanked; a crate whose releases are all yanked reports its newest release.

The account that first publishes a crate becomes its [owner](#package-owners).
`cargo owner` lists, adds, and removes owners.

## Python

Hosted wheel uploads expose their validated core metadata at
`<filename>.metadata`. The Simple API advertises its SHA-256 in JSON and HTML,
so installers can resolve dependencies without downloading the whole wheel.

A Python registry serves the Simple Repository API at `/pypi/simple/` (PEP 503
HTML and PEP 691 JSON, chosen by `Accept`), files at
`/pypi/files/<project>/<filename>`, and accepts uploads on the legacy API at
`/pypi/legacy/`. A named registry answers under `/pypi/~<name>/...`. The
account that first uploads to a project becomes its [owner](#package-owners).

```sh
pip install --index-url https://pnpr.example.com/pypi/simple/ my-package
twine upload --repository-url https://pnpr.example.com/pypi/legacy/ dist/*
```

Upload errors include the reason in the HTTP/1 status line, so twine displays
it without `--verbose`. Long reasons and reasons containing non-ASCII or
control characters remain in the response body. HTTP/2 carries the reason
only in the body.

Use `__token__` as the username and a pnpr token as the password, the PyPI
convention.

Project names are compared PEP 503 normalized, so `My_Package`, `my.package`,
and `my-package` are one project, and exact `packages:` keys are normalized the
same way. Proxied files are fetched from the URL the upstream page lists,
verified against its `sha256`, and cached. Upstream core-metadata declarations
(PEP 658/714) are preserved in JSON and HTML pages. Clients can download
`<filename>.metadata` through the proxy without downloading the distribution.
Metadata is cached and checked against a published SHA-256, SHA-384, or
SHA-512 digest when available. Hash declarations without a supported digest
are not advertised. Legacy metadata declarations are accepted,
and JSON pages expose them using `core-metadata` for compatibility with pip.
The project list at `/pypi/simple/` enumerates hosted projects only.

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

## Package owners

A hosted crate, Python project, or image repository records the accounts that
own it, starting with the account that first publishes it. Under the default
[`publish` rule](configuration.md#the-packages-map), only its owners and the
`auth.admins` can publish to it.

Owners and `auth.admins` can add and remove owners. Anyone who can read the
package can list them. A package keeps at least one owner.

| Ecosystem | Owners URL |
| --- | --- |
| Cargo | `/cargo/api/v1/crates/<crate>/owners` |
| Python | `/pypi/owners/<project>` |
| OCI | `/oci/owners/<repository>` |

A named registry answers under its base, such as
`/pypi/~<name>/owners/<project>`. Each URL takes the requests `cargo owner`
sends to crates.io:

- `GET` lists the owners: `{"users": [{"id": 1, "login": "alice", "name": "alice"}]}`.
- `PUT` with `{"users": ["bob"]}` adds owners by account name.
- `DELETE` with `{"users": ["alice"]}` removes them.

An owner whose account was deleted is listed by its account ID, and is removed
by that ID.

```sh
cargo owner --registry pnpr --add bob demo
curl -X PUT -H "Authorization: Bearer $PNPR_TOKEN" \
  -d '{"users":["bob"]}' https://pnpr.example.com/pypi/owners/my-package
```

## Proxying crates.io and PyPI

Cargo and Python proxies restrict downloads and redirects to their configured
upstream origin and the existing [`routes.public`](configuration.md#routes)
allowlist. The official crates.io and PyPI upstreams also permit their download
hosts, `static.crates.io` and `files.pythonhosted.org`. A custom registry that
serves downloads from a separate host needs that host declared:

```yaml title="registries.yaml"
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
as one generation of its namespace, so it lands whole or leaves nothing behind,
and a read sees the whole release or none of it:

- Losing a write race, to another writer publishing the same file between the
  check and the commit, fails the whole batch with `409`, and nothing is
  published. The bytes that won the slot are someone else's published release,
  and overwriting them would be the worse outcome. Republishing the batch after
  bumping the conflicting version completes the release.
- Every entry must belong to the same namespace. A batch that spans namespaces
  is refused.
- A retry gets the original response back while none of its published npm versions
  have been unpublished and none of its published OCI manifests have been deleted
  by digest. Retrying cannot restore an unpublished npm version. After deletion
  by digest, a retry is evaluated again. An unnamed batch can republish the OCI
  manifest, but a named batch can return `409` because its publication name is
  taken. A client sends an
  `Idempotency-Key` header, or the digest of the whole request serves as the
  key. Reusing a key for a different request answers `409`. pnpr keeps the
  outcome for 7 days. A retry needs current publish permission like any
  publish. A caller whose permission was revoked since gets `403` and can
  read the outcome from the publication or the package.
- The commit is recorded as a publication. A top-level `"name": "v2.0.0"`
  field in the body names it. The name must be unique within the namespace, at
  most 214 bytes long, and not a decimal number, which would read as a
  publication id.
  See [publication endpoints](endpoints.md#publication-endpoints).
- A top-level `"notes"` field is the publication's release note, and a
  `"notes"` field in an npm, Cargo, or Python entry is the note of the version
  it publishes. pnpr stores them with the release. A note breaking the
  [content rules](endpoints.md#release-note-endpoints) rejects the whole
  batch.

An [image manifest](container-images.md) whose layers are already uploaded can
ride along in the same batch. `GET /-/pnpr` advertises support as
`publish: [0]`.

Individual publishes to one package serialize their metadata updates. With a
remote database, each commit also waits for database round trips.
A publisher using the HTTP API can include several versions in one npm
publish document to commit them together. A batch accepts one document per
package.

### Staged archives {#staged-archives}

A publisher can upload each archive on its own before the publish that
references it, so one request body need not carry a whole release:

```text
PUT /-/pnpr/v0/publish/archives/sha512-…
```

The integrity is one percent-encoded path segment, so its `/`, `+`, and `=`
are sent as `%2F`, `%2B`, and `%3D`.

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
[database](storage.md#database). A sweep records an expired
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
