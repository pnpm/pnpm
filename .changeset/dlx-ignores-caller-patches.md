---
"pacquet": patch
---

`pnpm dlx` and `pnpm create` no longer fail with "Failed to read patch file" in a project that has `patchedDependencies`. As in pnpm, the package dlx runs is installed unpatched.
