---
"pacquet": patch
---

`pnpm update --no-save <pkg>@<version>` now keeps the manifest's declared importer specifier in `pnpm-lock.yaml` when the requested version satisfies that range, so a subsequent `--frozen-lockfile` install no longer fails because the lockfile records the requested version as the specifier.
