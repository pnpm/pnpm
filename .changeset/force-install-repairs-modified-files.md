---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install --force` now reinstalls dependencies when the manifest and lockfile are unchanged. It previously reported "Already up to date" without reinstalling. Files changed in `node_modules` are restored when the store content is intact. Combining `--force` with `--frozen-store` now reports the existing configuration conflict on repeat installs [#919](https://github.com/pnpm/pnpm/issues/919).
