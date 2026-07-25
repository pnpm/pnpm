---
"pacquet": patch
---

Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches both the packed manifest and the tarball filename. This also fixes the changelog of the Rust CLI itself, which is published as `pnpm` from a workspace project named `pacquet`: its release notes were composed under the workspace name and so never made it into the published package [#13345](https://github.com/pnpm/pnpm/issues/13345).
