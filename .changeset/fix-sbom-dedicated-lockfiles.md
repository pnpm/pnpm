---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

`pnpm sbom` now reads project lockfiles in workspaces with `sharedWorkspaceLockfile: false`, including filtered and split output.
