---
"@pnpm/installing.deps-installer": patch
"@pnpm/deps.inspection.peers-checker": patch
"pnpm": patch
"pacquet": patch
---

Fixed `peerDependencyRules.ignoreMissing` not suppressing peer dependency errors when the peer is found but doesn't satisfy the required version range. Previously `ignoreMissing` only suppressed truly absent peers; it now also covers peers that resolve to a version outside the wanted range for the peer edge it selects.
