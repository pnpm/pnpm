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
- Packuments serve the verified variants as `dist.pnpr.variants`, and each
  variant's tar is served at `<name>@<version>/variants/<tag>`.

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

- pnpm asks for `application/vnd.pnpr.install-v2+json` first. pnpr answers
  with the abbreviated packument. When that is larger than
  `PNPR_SPLIT_METADATA_THRESHOLD` (off unless set; the benchmark uses
  64 KiB), only the likely picks stay in `versions`: every dist-tag target
  and the highest release of each major (of each minor below 1.0;
  `PNPR_SPLIT_KEEP_LATEST=0` turns this off). Every other version moves to
  `pnpr.otherVersions` as an object with its `deprecated` notice, trust
  rank, and selected revision, and its publish `time` only if it was
  published within 30 days of the package's last publish
  (`PNPR_SPLIT_TIME_WINDOW_DAYS` makes the window longer, never shorter). `pnpr.timesSince`
  is where that window starts. A client that does not read the map sees only
  complete versions.
- The rest of a version is its release record at
  `<name>/<version>+r<revision>`, served with `Cache-Control: immutable`:
  the abbreviated version without `deprecated`, `_npmUser`, signatures, and
  attestations, which may change or are evidence kept apart. `<name>/<version>`
  is the selected revision and keeps its usual caching. The PoC serves a
  record only while its revision is the selected one.
- pnpm turns each entry into a version marked `dist.pnpr.details: "external"`
  where the document is parsed, so the picker, the trust policy, and the
  release-age filter read it like any other. A version without a time is
  recorded as published at `timesSince`, which passes any minimum release
  age up to 30 days and is what time-based resolution compares too. pnpm does not yet reject a longer one, which the RFC
  requires. It fetches the record of a picked
  other version once per install and takes the deprecation and trust
  evidence from the packument. Records are not yet cached on disk.
- `PNPR_SPLIT_DIST_ONLY=1` keeps every version in `versions` with only its
  `dist` moved, for comparison.
- Split documents need a metadata cache of their own on the client: an older
  pnpm on `PATH` that shares the cache read a split document the new one had
  cached, and failed to switch versions. Moving the cache directory touches
  the cache commands and many tests, so the PoC leaves splitting off unless
  `PNPR_SPLIT_METADATA_THRESHOLD` is set.

`bench-split.sh` resolves `bench-app.json` (787 packages, all proxied from
npmjs) through `shaper.mjs`, a CDN-like proxy that gzips, caches, delays each
request by a round trip, and caps bandwidth. The registry is declared with
`serverType: pnpr`, as it would be in use; otherwise pnpm reads the full
document of every optional dependency, which is never split. Every run starts
with an empty pnpm cache and store. Both split forms keep the likely picks
whole. Medians of 3 runs:

| Network | Whole | Only `dist` moved | Map of other versions |
| --- | --- | --- | --- |
| 10 ms, 200 Mbit/s, resolve | 1.36 s | 0.45 s | 0.32 s |
| 40 ms, 50 Mbit/s, resolve | 5.05 s | 1.51 s | 1.01 s |
| 100 ms, 10 Mbit/s, resolve | 24.66 s | 6.67 s | 4.03 s |
| 40 ms, 50 Mbit/s, install | 28.48 s | 24.82 s | 24.27 s |
| Packument bytes (gzip) | 30.3 MB | 7.6 MB | 4.3 MB |
| Release records | 0 | 22 (7 KB) | 22 (7 KB) |

All three lockfiles are identical. About 3 MB of the 4.3 MB left is in
documents under the threshold.

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
