---
"@pnpm/workspace.read-manifest": minor
---

The `readWorkspaceManifest` function now parses [pnpm catalogs](https://github.com/pnpm/rfcs/pull/1) configs if given an options object with the `catalogs` property set to `true`. This field will default to `false` until the overall catalogs feature is fully implemented in pnpm.
