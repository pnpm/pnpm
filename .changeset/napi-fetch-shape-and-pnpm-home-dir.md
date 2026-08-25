---
"@pnpm/napi": minor
"pacquet": minor
---

`@pnpm/napi`'s `install` now honors the last two install options it accepted without acting on them:

- `ignorePackageManifest: true` installs from `pnpm-lock.yaml` alone, ignoring the project manifests — pnpm's `pnpm fetch` semantics. Every importer the lockfile records is imported into the virtual store, and no post-import linking is performed: no importer symlinks, no `.bin` entries, no hoisting, and no project lifecycle scripts. It previously only skipped the manifest ↔ lockfile freshness check and otherwise linked a full `node_modules`.
- `pnpmHomeDir` now places the default store at `<pnpmHomeDir>/store`, with the same same-volume fallback pnpm applies. An explicit `storeDir` — passed alongside it or set by a config source — still wins. It was previously ignored.
