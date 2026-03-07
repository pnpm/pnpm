---
"@pnpm/calc-dep-state": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/resolve-dependencies": minor
"pnpm": minor
---

Use `allowBuilds` config to compute engine-agnostic GVS hashes for pure-JS packages [#10837](https://github.com/pnpm/pnpm/issues/10837).

When the global virtual store is enabled, packages that are not allowed to build (and don't transitively depend on packages that are) now get hashes that don't include the engine name (platform, architecture, Node.js major version). This means ~95% of packages in the GVS survive Node.js upgrades and architecture changes without re-import.
