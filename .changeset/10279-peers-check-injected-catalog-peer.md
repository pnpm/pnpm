---
"@pnpm/deps.inspection.peers-checker": patch
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
---

`pnpm peers check` now resolves the `catalog:` peer ranges of injected workspace packages through the workspace catalogs. With `autoInstallPeers: false`, such a peer was reported as unmet even when the installed version matched the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).
