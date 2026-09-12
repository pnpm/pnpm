---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

Under `resolvePeersFromWorkspaceRoot`, a workspace root dependency declared with `link:` or `file:` (or the path form of `workspace:`, such as `workspace:../pkg`) now satisfies another project's missing peer dependency at the linked package's own version, instead of being hoisted as a path. Those specifiers are relative to the project that declares them, so the same specifier reached a different directory — or none — from the project the peer was hoisted into, leaving a broken link. The root now has the same authority over the peer as it has when it declares the package with a version range [#13373](https://github.com/pnpm/pnpm/issues/13373).
