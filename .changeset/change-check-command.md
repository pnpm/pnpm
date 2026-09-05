---
"@pnpm/releasing.versioning": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
"pacquet": minor
---

Added `pnpm change check`. It validates the committed package versions against the `versioning.epics` bands and the `versioning.fixed` groups in `pnpm-workspace.yaml` and lists every violation. It is meant to run in CI, because `pnpm version -r` only checks the packages it releases.
