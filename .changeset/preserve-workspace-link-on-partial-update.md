---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Preserve a workspace dependency's `link:` entry when an unrelated dependency is updated recursively (e.g. `pnpm update <pkg> --recursive`) with `injectWorkspacePackages`, instead of spuriously rewriting it to a peer-suffixed `file:` protocol. See pnpm/pnpm#10433.
