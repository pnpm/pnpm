# Platform variants proof of concept

A prototype of the pnpr platform-variants RFC (pnpm/rfcs#51): one package
version with a primary archive and platform variant archives.

## What is implemented

pnpr:

- A publish body declares variants under `versions[<v>].pnprVariants`
  (`tag`, `platforms`, `.tgz` `integrity`) and attaches each one as
  `<basename>-<version>~<tag>.tgz`.
- Publication rejects a variant whose `name` or `version` differs from the
  primary, whose dependency fields differ after removing bundled names, that
  requires a build, whose `os`/`cpu`/`libc` excludes a declared platform,
  that lists a platform another variant lists, or that is declared but not
  attached (or attached but not declared).
- pnpr's own package document (see [Split packuments](#split-packuments))
  and its release records list the verified variants as `variants`; npm's
  abbreviated, full, and version documents do not, so npm clients see only
  the primary. Each variant's tar is served at
  `<name>@<version>/variants/<tag>`.

pnpm:

- The npm resolver turns `dist.pnpr.variants` into a `variations` resolution
  with a `tag` per variant and the primary as `fallback`. Variants on their
  registry route are written integrity-only.
- Runtimes (Node.js, Bun, Deno) and registry packages share one
  `variations` selection: the first variant whose targets include the
  platform `supportedArchitectures` selects (the host by default), else the
  `fallback`. The selected resolution's type picks the fetcher: a `binary`
  archive goes through the runtime extractor, and a registry variant is
  fetched from its route like a revision (verified against its tar digest).
- The global virtual store hashes every `variations` entry, runtime or
  package, by the selected archive's integrity, with the platform passed
  down from the install's config.

Not implemented: staged publishing, the `libcVersion`, `osVersion`, `napi`,
`nodeAbi`, and `wasm32` fields, the compatibility view for other clients,
fetching several variants for `supportedArchitectures`, and relinking a
local virtual store slot when the selection changes.

## Split packuments

Variant integrities make a packument several times larger, because a digest
does not compress. So the PoC also implements split packuments:

- pnpm asks a registry declared `serverType: pnpr` for
  `application/vnd.pnpr.install-v2+json` first, and pnpr answers with a
  document in its own format (`PNPR_INSTALL_V2=0` turns it off). Other
  registries, and pnpr when it is not declared, are asked for npm's
  documents. The format has: `name`, `distTags`,
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
`mkinvariants.mjs` builds packages that each break one invariant.
