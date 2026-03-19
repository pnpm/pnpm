---
"@pnpm/building.commands": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": minor
"pnpm": minor
---

Allow `pnpm approve-builds` to receive positional arguments for approving the listed packages without the interactive prompt.

During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added as `pending`. This makes them visible in `pnpm-workspace.yaml` so users can manually change them to `true` or `false` without running `pnpm approve-builds`.
