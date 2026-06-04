---
"@pnpm/installing.deps-resolver": minor
"pnpm": minor
---

Peer dependency resolution now reuses the peer contexts already recorded in the lockfile when those providers are still present in the dependency graph and still satisfy the peer ranges. This avoids unnecessary peer-context rewrites during lockfile regeneration. Current manifest choices remain authoritative: a newly added, explicitly updated, or aliased direct provider, a changed nested provider, or a locked version that no longer satisfies the range still takes precedence.
