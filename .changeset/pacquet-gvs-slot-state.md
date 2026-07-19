---
"pacquet": patch
---

Fixed two global-virtual-store correctness gaps. A failed build now discards the hash directory it was building in, so the next install re-fetches instead of reusing a half-built directory shared by every project with the same dependency graph. The removal only ever touches a slot strictly inside the store, so a crafted package name cannot make it escape. And a side-effects-cache hit no longer assumes the store slot still holds the cached build: when the slot has been re-imported pristine, the build output is materialized rather than skipped, which previously left the package without its build artifacts.

`.modules.yaml` now records the `allowBuilds` set the install ran under, matching pnpm.
