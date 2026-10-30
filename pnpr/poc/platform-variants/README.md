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
