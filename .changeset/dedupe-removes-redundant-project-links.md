---
"pacquet": patch
---

With `dedupeDirectDeps`, a project's symlink that becomes redundant — because the workspace root started providing the same dependency at the same resolution — is removed on the next install instead of surviving forever [#13775](https://github.com/pnpm/pnpm/issues/13775). The layout no longer depends on install history: an incremental install now ends up with the same `node_modules` a clean install of the same manifests produces.
