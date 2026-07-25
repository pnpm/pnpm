---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

Under `resolvePeersFromWorkspaceRoot`, a workspace root dependency declared with `link:` or `file:` (or the path form of `workspace:`, such as `workspace:../pkg`) is no longer used to satisfy another project's missing peer dependency. Those specifiers are relative to the project that declares them, so the same specifier reached a different directory — or none — from the project the peer was hoisted into. A `workspace:` range such as `workspace:^8.5.10` selects the same workspace package from every project, so it keeps satisfying peers [#13373](https://github.com/pnpm/pnpm/issues/13373).
