---
"pacquet": minor
---

pnpm can now install from a registry that serves uncompressed tars with zstd or gzip content coding. `pnpm-lock.yaml` records the hash of each tar. Declare such a registry with `serverType: pnpr` in `registries` to keep tarball URLs out of the lockfile. See [Tar registries](https://pnpm.io/registries#tar-registries).

pnpm now decodes the content coding of tarball downloads itself. It rejects a zstd response whose window is larger than 8 MiB.

Added the `maxTarballUnpackedSize` setting. It caps the bytes the files of one package may unpack to, 4 GiB by default.
