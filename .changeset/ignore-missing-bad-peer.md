---
"@pnpm/installing.deps-installer": minor
"@pnpm/deps.inspection.peers-checker": minor
"pnpm": minor
"pacquet": minor
---

`peerDependencyRules.ignoreMissing` now suppresses every peer dependency issue for a matched peer, including when the peer is present but resolves to a version outside its wanted range. Use `peerDependencyRules.allowAny` to accept any resolved version while still being told about absent peers.
