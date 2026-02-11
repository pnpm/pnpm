---
"@pnpm/core": patch
"pnpm": patch
---

Decoupled `shouldForceResolve` from `canResolve` in custom resolvers. `shouldForceResolve` is now called for every package in the lockfile without first gating on `canResolve`, since it runs before resolution where the original specifier is not available. Resolvers should handle their own filtering within `shouldForceResolve` (e.g. by inspecting `depPath` or `pkgSnapshot.resolution`).
