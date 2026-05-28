---
"@pnpm/installing.linking.real-hoist": minor
"@pnpm/config.reader": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new `hoistingLimits` setting for `nodeLinker: hoisted` installs, mirroring yarn's `nmHoistingLimits`. It accepts `none` (the default — hoist as far as possible), `workspaces` (hoist only as far as each workspace package), or `dependencies` (hoist only up to each workspace package's direct dependencies). Originally proposed in [#6468](https://github.com/pnpm/pnpm/pull/6468), closing [#6457](https://github.com/pnpm/pnpm/issues/6457).
