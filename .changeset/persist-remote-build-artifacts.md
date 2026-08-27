---
"@pnpm/store.cafs-types": minor
"@pnpm/store.cafs": minor
"@pnpm/store.controller-types": minor
"@pnpm/store.controller": patch
"@pnpm/store.index": minor
"@pnpm/worker": patch
"@pnpm/pnpr.client": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
"pacquet": minor
---

Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, source, and lockfile pins before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).
