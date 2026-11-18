---
"@pnpm/deps.inspection.peers-checker": patch
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm peers check` and the peer dependency warning of `pnpm install` now resolve the `catalog:` peer ranges of injected workspace packages through the workspace catalogs. With `autoInstallPeers: false`, such a peer was reported as unmet even when the installed version matched the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).
