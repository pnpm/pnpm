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
  64 KiB), each version's `dist`
  moves to its immutable version document `<name>/<version>` and is marked
  `dist.pnpr.details: "external"`. Only provenance and each variant's `tag`
  and `platforms` stay inline. A missing `integrity` cannot be the marker,
  because it already means the integrity is computed while fetching the
  tarball.
- Every dist-tag target and the highest release of each major (of each minor
  below 1.0) keep their `dist` inline (`PNPR_SPLIT_KEEP_LATEST=0` turns this
  off), so most picks need no second request.
- pnpm fetches the version document of a picked version marked external, once
  per install. Version documents are not yet cached on disk.
- Split documents need a metadata cache of their own on the client: an older
  pnpm on `PATH` that shares the cache read a split document the new one had
  cached, and failed to switch versions. Moving the cache directory touches
  the cache commands and many tests, so the PoC leaves splitting off unless
  `PNPR_SPLIT_METADATA_THRESHOLD` is set.

`bench-split.sh` resolves `bench-app.json` (787 packages, all proxied from
npmjs) through `shaper.mjs`, a CDN-like proxy that gzips, caches, delays each
request by a round trip, and caps bandwidth. Every run starts with an empty
pnpm cache and store. Medians of 3 runs:

| Network | Whole | Split, every version | Split, likely picks inline |
| --- | --- | --- | --- |
| 10 ms, 200 Mbit/s, resolve | 1.71 s | 1.15 s | 1.21 s |
| 40 ms, 50 Mbit/s, resolve | 6.67 s | 4.10 s | 3.98 s |
| 100 ms, 10 Mbit/s, resolve | 31.89 s | 19.06 s | 19.04 s |
| 40 ms, 50 Mbit/s, install | 29.89 s | 27.32 s | 27.40 s |
| Packument bytes (gzip) | 39.2 MB | 22.5 MB | 23.0 MB |
| Version documents | 0 | 180 (278 KB) | 14 (20 KB) |

180 of the 711 packuments are over the threshold, and they hold most of the
bytes. The 9 `@next/swc-*` platform packages alone are about 1 MB each, even
split, since they list thousands of canary versions; as variants of one
package they would be one document.

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
