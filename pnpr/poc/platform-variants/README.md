# Platform variants proof of concept

<!-- cspell:words mkinvariants mkvariants mktool typst Mbit -->

A prototype of the
[pnpr platform-variants RFC](https://github.com/pnpm/rfcs/blob/main/pnpr/text/0000-platform-variants.md):
one package version with a primary archive and platform variant archives.

## What is implemented

pnpr:

- A variant archive declares the platforms it runs on in its own
  `package.json`, as platform tags: `"variant": { "platforms":
  ["linux-x64", "linux-x64-musl"] }`. The tags cover `os`, `cpu`, musl,
  glibc floors (`manylinux_2_28`), macOS and Windows floors (`macos_11_0`),
  N-API (`napi8`), Node.js and Electron ABIs (`node127`), and `wasm32`
  (the shared `pnpm-platform-variants` crate); only the canonical form
  parses. pnpr names the variant by its platforms' tags, sorted and joined
  with `+`, such as `linux-x64+linux-x64-musl`.
- The publish body references each variant archive by the `.tgz` integrity
  its upload is verified against, under `versions[<v>].pnprVariants` as
  `{ "<label>": "<integrity>" }`, and attaches it as
  `<basename>-<version>~<label>.tgz`. The label only pairs a reference with
  its attachment. Uploads by integrity before a separate commit request
  (atomic workspace publication) are not implemented; the archives travel
  in one publish body.
- Publication rejects a variant whose `name`, `version`, `peerDependencies`,
  or `peerDependenciesMeta` differ from the version's, that requires a
  build, that has no `variant` field or one that does not parse, that
  declares `os`, `cpu`, or `libc`, or that lists a platform another variant
  lists; a primary with a `variant` field; and a variant referenced but not
  attached, or attached but not referenced. Variants may differ in their
  dependencies, bundled dependencies, `bin`, `main`, `exports`, and
  `engines`.
- pnpr's own package document (see [Split packuments](#split-packuments))
  and its release records list the verified variants as `variants`: each
  with its `tag`, structured `platforms`, tar `integrity`, sizes, `bins`,
  and a `manifest` of only the fields that differ from the version's
  (`null` for a removed field). npm's documents do not list them. Each
  variant's tar is served at `<name>@<version>/variants/<tag>`, and at
  `<name>@<version>+r<revision>/variants/<tag>` while that revision is the
  selected one.

pnpm:

- The npm resolver turns pnpr's `variants` into a `variations` resolution
  with the primary as `fallback`, ordered best first. The lockfile stores no
  tag: pnpm derives it from each variant's `targets`, which carry the floor
  and ABI fields. A revision-aware version records `revision` on the
  `variations` resolution, and its variants are fetched from their `+r<N>`
  route. A variant with a platform field pnpm does not know is left out.
- A package's variant is selected by the RFC's rule: more matched fields,
  then the highest glibc, OS, and N-API floors, then `wasm32`, then the
  primary; two variants that fit equally well fail the install. pnpm
  probes the host's glibc, macOS, N-API, and ABI versions once. Runtimes
  (Node.js, Bun, Deno) keep their first-match selection.
- Every archive's dependencies are resolved on every machine. The version's
  snapshot keeps those its primary declares, and a variant whose resolved
  snapshot differs gets `<name>@<version>/variants/<tag>(<peers>)`. Before
  materializing the lockfile, the installer wires each package with its
  selected variant's snapshot.
- The global virtual store hashes every `variations` entry, runtime or
  package, by the selected archive's integrity.

`variants.sh` publishes the `mkinvariants.mjs` packages to a hosted pnpr,
shows which are rejected and why, and installs the accepted ones from pnpr's
format, frozen and after a `pnpm add`: the host's variant is linked, and a
dependency only the Linux variant declares is linked for it alone.

Not implemented:

- a version without a primary, and the minimal primary archive pnpr would
  generate for npm clients;
- a name that two archives declare with different specifiers: it resolves
  once, with the version's specifier;
- `supportedArchitectures` fetching several variants, and linking the one
  platform it names: the materialized dependencies follow the host;
- relinking a package when its selected variant changes;
- `pnpm why`, `audit`, `licenses`, and SBOM generation visiting the
  variants' snapshots;
- a lockfile marker that older pnpm releases reject;
- uploads by integrity, one provenance statement naming every archive, and
  the compatibility view for other clients.

## Split packuments

Variant integrities make a packument several times larger, because a digest
does not compress. So the PoC also implements split packuments:

- pnpm asks a registry declared `serverType: pnpr` for
  `application/vnd.pnpr.install-v2+json` first, and pnpr answers with a
  document in its own format (`PNPR_INSTALL_V2=0` turns it off). Other
  registries, and pnpr when it is not declared, are asked for npm's
  documents. pnpr sends a `pnpr-registry` header with package documents; the
  RFC has pnpm warn when an undeclared registry sends it, which the PoC does
  not do yet, since pnpm's own tests run against an undeclared pnpr and
  compare their output exactly. The format has: `name`, `distTags`,
  `modified`, `timesSince`, and an entry per version in `versions` with its
  `deprecated` notice, `trust` rank, selected `revision`, and its publish
  `time` only if it was published within 30 days of the package's last
  publish (`PNPR_SPLIT_TIME_WINDOW_DAYS` makes the window longer, never
  shorter). `timesSince` is where that window starts.
- An entry with a `manifest` is complete: beside the publisher's manifest it
  has pnpr's facts about the archives, `integrity`, `fileCount`,
  `unpackedSize`, `requiresBuild`, `bins`, and `variants`. npm's documents
  carry none of pnpr's facts, not even `dist.pnpr`. Under
  `PNPR_SPLIT_METADATA_THRESHOLD` (64 KiB by default) every entry has a
  manifest; above it only the likely picks do: every dist-tag target and the
  highest release of each major (of each minor below 1.0;
  `PNPR_SPLIT_KEEP_LATEST=0` turns this off).
- Any version's facts and manifest are its release record at
  `<name>/<version>+r<revision>`, `{ name, version, integrity, ...,
  manifest }`, served with `Cache-Control: immutable`. It leaves out
  `deprecated`, `_npmUser`, signatures, and attestations, which may change or
  are evidence kept apart. `<name>/<version>`
  is the selected revision and keeps its usual caching. The PoC serves a
  record only while its revision is the selected one.
- pnpm turns the document into its usual package type where it is parsed
  (`parse_install_v2`), with a version marked `dist.pnpr.details: "external"`
  for each entry without a manifest, so the picker, the trust policy, the
  release-age filter, and the metadata cache read it like any other. A
  version without a time is recorded as published at `timesSince`, which
  passes any minimum release age up to 30 days and is what time-based
  resolution compares too. pnpm does not yet reject a longer one, which the
  RFC requires. It fetches the record of a picked version without a manifest
  once per install and takes the deprecation and trust evidence from the
  document. Records are not yet cached on disk.
- pnpr's documents are cached in `v12/metadata-pnpr`, apart from npm's in
  `v12/metadata`: an older pnpm on `PATH` that shared the cache read a split
  document the new one had cached, and failed to switch versions. The
  `pnpm cache` commands do not list the new directory yet.

### Revisions

The revision protocol's fields are only in pnpr's format too. npm's documents
give a selected revision the way npm clients read one: its `+r<N>` tarball
URL and that tar's integrity, without `dist.revision` and `dist.revisions`.
pnpr's format marks a revision-aware version with `revision`, including `0`,
and gives no history: every revision in the history has its release record
at `<name>/<version>+r<N>`. pnpm rebuilds the `+r<N>` URL from `revision`,
reads a `+r<N>` override's record when it is not the selected revision, and
treats an unmarked revision URL in npm's documents as an ordinary tarball
that answers only for its own revision.

`revisions.sh` serves an original and a patched revision from a small
upstream (`revision-upstream.mjs`) through pnpr. Declared `serverType: pnpr`,
pnpm installs the patched revision and records `revision: 1`; undeclared, it
installs it from npm's document and records the URL; a `+r0` override
installs the original when declared and fails when not, since npm's document
cannot name another revision. Every install that succeeds is repeated frozen.

`bench-split.sh` resolves `bench-app.json` (787 packages, all proxied from
npmjs) through `shaper.mjs`, a CDN-like proxy that gzips, caches, delays each
request by a round trip, and caps bandwidth. The registry is declared with
`serverType: pnpr`, as it would be in use; otherwise pnpm reads the full
document of every optional dependency. Every run starts with an empty pnpm
cache and store. It compares npm's abbreviated documents, pnpr's format with
every manifest inline, and pnpr's format with the 64 KiB threshold. Medians
of 3 runs:

| Network | npm's abbreviated | pnpr's, every manifest | pnpr's, 64 KiB threshold |
| --- | --- | --- | --- |
| 10 ms, 200 Mbit/s, resolve | 1.35 s | 0.76 s | 0.36 s |
| 40 ms, 50 Mbit/s, resolve | 5.07 s | 2.60 s | 0.79 s |
| 100 ms, 10 Mbit/s, resolve | 24.68 s | 12.38 s | 2.62 s |
| 40 ms, 50 Mbit/s, install | 28.50 s | 25.99 s | 24.03 s |
| Package documents (gzip) | 30.3 MB | 14.9 MB | 2.5 MB |
| Release records | 0 | 0 | 22 (7 KB) |

All three lockfiles are identical. The format alone halves the bytes: it
leaves out npm's registry signatures, attestations, `_npmUser`, publish
times older than the window, and the repeated `name` and `version`.

## Reproducing

```sh
pnpr -c pnpr.yaml --listen 127.0.0.1:7690
# create a user (PUT /npm/-/user/org.couchdb.user:<name>) and keep its token
node mkvariants.mjs esbuild 0.28.2 esbuild out
node publish.mjs http://127.0.0.1:7690/npm/ <token> out/esbuild-0.28.2.variants.json
```

Adapters in `mkvariants.mjs`: `esbuild`, `biome`, `napi`,
`napi-wasm-primary` (`@tailwindcss/oxide`), `lightningcss`, `rolldown`.
`mktool.mjs` repackages GitHub release archives (`typst-assets.json`).
`mkinvariants.mjs` builds packages that each break one invariant, and
`variants.sh` publishes and installs them.
