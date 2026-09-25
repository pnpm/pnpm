---
"@pnpm/releasing.exportable-manifest": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm publish` again sends the package's README to the registry as metadata, so registries can render it on the package page. The readme is always included in the published metadata (matching the npm CLI), while the `embed-readme` setting continues to control only whether the readme is written into the `package.json` inside the tarball. This restores the behavior that was lost when publishing became fully native. Closes pnpm/pnpm#12966.
