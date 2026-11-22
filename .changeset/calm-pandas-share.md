---
"@pnpm/pnpr": minor
"pnpm": minor
---

Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.
