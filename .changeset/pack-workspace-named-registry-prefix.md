---
"@pnpm/config.pick-registry-for-package": minor
"@pnpm/config.reader": patch
"@pnpm/releasing.exportable-manifest": minor
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": minor
---

`pnpm pack` and `pnpm publish` now write a `workspace:` dependency with the named-registry prefix of the registry the target package publishes to. With `prefix: work` declared for that registry in `pnpm-workspace.yaml`, `"foo": "workspace:^"` is published as `"foo": "work:^1.0.0"`, and a consumer that installs `foo` through the prefix records one lockfile entry for it. Peer dependency ranges stay plain semver ranges. When several prefixes declare the same registry, the first in alphabetical order is used.
