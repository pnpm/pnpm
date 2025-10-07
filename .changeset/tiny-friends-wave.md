---
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/outdated": patch
"pnpm": patch
---

`pnpm dlx` and `pnpm outdated` should request the full metadata of packages, when `minimumReleaseAge` is set [#9963](https://github.com/pnpm/pnpm/issues/9963).
