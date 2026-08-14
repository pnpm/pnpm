---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Removing a package from a workspace no longer forces a full re-resolution. The lockfile update drops the departed project's importer entry and prunes whatever only it depended on. A project that is still linked from a surviving project continues to be reported as an error [#13696](https://github.com/pnpm/pnpm/issues/13696).
