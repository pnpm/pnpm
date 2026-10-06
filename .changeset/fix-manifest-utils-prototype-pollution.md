---
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
---

`updateProjectManifestObject` now validates `saveType` against allowed dependency fields to prevent prototype pollution [pnpm/tasks#91](https://github.com/pnpm/tasks/issues/91).
