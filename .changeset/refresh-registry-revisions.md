---
"@pnpm/pnpr": minor
"pacquet": minor
---

Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.
