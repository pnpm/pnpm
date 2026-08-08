---
"pacquet": patch
---

`pnpm dlx` and `pnpm create` work again in a project that has `patchedDependencies`. The caller's patches were carried into the throwaway cache install, whose patch paths no longer resolve, so every invocation failed with "Failed to read patch file". The cache install now ignores them, as pnpm does.
