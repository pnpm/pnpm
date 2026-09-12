---
"@pnpm/deps.graph-hasher": minor
"@pnpm/types": minor
"@pnpm/config.reader": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/building.during-install": minor
"@pnpm/store.controller-types": minor
"@pnpm/store.controller": minor
"@pnpm/worker": minor
"@pnpm/pnpr.client": minor
"@pnpm/pnpr": minor
"pnpm": minor
"pacquet": minor
---

Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.
