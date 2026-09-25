---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm update --recursive <pkg>` no longer changes the version of a peer dependency that another workspace project installs automatically. Such a peer could move to a version outside the range the project declares, for example to React 19 in a project that declares `react: ^18.3.1` [#14928](https://github.com/pnpm/pnpm/issues/14928).
