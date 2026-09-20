---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install --force` now reinstalls dependencies when the manifest and lockfile are unchanged. It previously reported "Already up to date" without reinstalling. pnpm restores files that changed in `node_modules`, and refetches the package when the change also reached the store. Combining `--force` with `--frozen-store` now reports a configuration conflict on repeat installs [#919](https://github.com/pnpm/pnpm/issues/919).
