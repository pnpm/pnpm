---
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

With `enableGlobalVirtualStore` and `nodeExperimentalPackageMap`, `node_modules/.package-map.json` no longer contains entries pointing at non-existent flat virtual-store paths. A metadata-only entry (`name@version`, which carries no peer/patch suffix) is not one of the snapshot keys the global virtual store precomputes hashed slots for, so resolving it directly fell back to the legacy flat name. It now resolves its slot through a peer-suffixed snapshot sibling, which holds the same package version at a real directory.
