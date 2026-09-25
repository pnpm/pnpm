---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
---

Directory filters such as `--filter=./packages/*` now select projects when the current directory was entered with a lowercase drive letter on Windows, like `c:\repo` [#5500](https://github.com/pnpm/pnpm/issues/5500).
