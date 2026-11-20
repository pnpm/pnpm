---
"get-pnpm": patch
---

The installer now reads the pnpm version a project pins from the manifest that the workspace's `preferredManifestFormat` setting selects.

The installer now matches `packages` patterns that contain `.` segments, such as `./packages/./*`, the way pnpm does.
