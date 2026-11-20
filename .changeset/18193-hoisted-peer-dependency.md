---
"@pnpm/installing.linking.real-hoist": patch
"pnpm": patch
---

Fixed `nodeLinker: hoisted` installs omitting a dependency that its parent also declares as a peer dependency when `autoInstallPeers` is disabled [#18193](https://github.com/pnpm/pnpm/issues/18193).
