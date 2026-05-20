---
"pnpm": patch
---

Pacquet's install path now honors `auto-install-peers` correctly. Previously, when enabled (the default), missing peer dependencies were folded in as nested children of every consumer; now they're hoisted to the importer's direct dependencies via the same `hoistPeers` algorithm pnpm uses, including the multi-pass loop that resolves transitively-required peers and the optional-peer pass that picks already-available versions from the preferred-versions map. The new `auto-install-peers-from-highest-match` setting (mirroring upstream's flag) controls range merging when multiple consumers declare conflicting peer ranges.
