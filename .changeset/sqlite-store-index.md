---
"@pnpm/store.index": minor
"@pnpm/store.cafs": minor
"@pnpm/worker": minor
"@pnpm/plugin-commands-store-inspecting": minor
"@pnpm/plugin-commands-store": minor
"@pnpm/package-store": minor
"@pnpm/store.pkg-finder": minor
"@pnpm/reviewing.dependencies-hierarchy": minor
"@pnpm/plugin-commands-rebuild": minor
"pnpm": minor
---

Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).
