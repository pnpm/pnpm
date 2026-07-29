---
"pacquet": patch
---

Installs driven through `@pnpm/napi` got three fixes for large workspaces: the `readPackage` hook is now dispatched to JavaScript in batches instead of one event-loop roundtrip per manifest, the `dedupePeers` setting can be passed through the install options (so an existing lockfile generated with it is no longer treated as outdated), and version-pinned dependencies are served from the metadata mirror without queueing behind concurrent registry revalidations of the same package.
