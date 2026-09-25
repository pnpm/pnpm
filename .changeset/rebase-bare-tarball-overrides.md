---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
---

Relative local tarball paths in `pnpm.overrides` without an explicit `file:` prefix are now rebased correctly for workspace packages [#11131](https://github.com/pnpm/pnpm/issues/11131).
