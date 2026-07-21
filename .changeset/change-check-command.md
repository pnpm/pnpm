---
"@pnpm/releasing.versioning": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
"pacquet": minor
---

Added `pnpm change check`, which validates that the committed package versions satisfy the `versioning.epics` major-band and `versioning.fixed` shared-version rules configured in `pnpm-workspace.yaml`, failing with every violation listed. The release engine only enforces these when a package actually releases; `pnpm change check` validates the whole workspace up front, so it can run in CI to catch version drift before a release reaches the registry.
