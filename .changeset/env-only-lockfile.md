---
"@pnpm/lockfile.fs": patch
"@pnpm/installing.context": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now succeeds in a project with no dependencies when `pnpm-lock.yaml` records only the pinned pnpm version. Other commands write such a lockfile when they run before the first install. A lockfile missing the `---` line after that section is accepted too [#16477](https://github.com/pnpm/pnpm/issues/16477).
