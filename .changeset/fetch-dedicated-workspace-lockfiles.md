---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm -r fetch` skipping project lockfiles when `sharedWorkspaceLockfile` is `false`. Filtered fetches now populate the store from the selected projects' lockfiles [pnpm/pnpm#18152](https://github.com/pnpm/pnpm/issues/18152).
