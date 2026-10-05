---
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

pnpm now verifies locked config dependencies against their registry before installing them. Config dependencies must come from an npm registry. The lockfile can no longer replace the integrity of a config dependency pinned with `version+integrity`.
