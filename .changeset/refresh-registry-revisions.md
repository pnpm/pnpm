---
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/pnpr": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.registry.types": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/store.controller-types": minor
"pacquet": minor
"pnpm": minor
---

Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.
