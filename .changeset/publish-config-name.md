---
"@pnpm/releasing.exportable-manifest": minor
"@pnpm/releasing.commands": minor
"@pnpm/types": minor
"pnpm": minor
---

Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. It is for a project whose published name is already taken by a sibling project, which otherwise has to be renamed by a build step just before publishing. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches the packed manifest, the tarball filename, and everything that addresses the package at the registry: the already-published check of `pnpm publish -r`, its registry selection, and the release-planning probes of `pnpm change status` and `pnpm version -r` [#13345](https://github.com/pnpm/pnpm/issues/13345).
