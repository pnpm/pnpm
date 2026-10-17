---
"pacquet": minor
---

pnpm can now install from a registry that serves uncompressed tars with zstd or gzip content coding, and pins the hash of each tar in `pnpm-lock.yaml`. Declare the registry with `serverType: pnpr` in `registries`, so the lockfile keeps omitting tarball URLs. See [Tar registries](https://pnpm.io/registries#tar-registries).

pnpm now decodes the content coding of tarball downloads itself, and rejects a zstd response whose window is larger than 8 MiB.

Added the `maxTarballUnpackedSize` setting. It caps the bytes the files of one package may unpack to, 4 GiB by default.
