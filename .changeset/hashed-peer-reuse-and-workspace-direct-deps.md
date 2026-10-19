---
"pacquet": patch
---

`pnpm install` and `pnpm dedupe` now preserve own-dependency and peer-dependency bindings when reusing a lockfile whose peer suffixes were hashed by `peersSuffixMaxLength`.

Upgrading a direct dependency in one workspace project now refreshes matching transitive pins in sibling projects during the same `pnpm install` pass.
