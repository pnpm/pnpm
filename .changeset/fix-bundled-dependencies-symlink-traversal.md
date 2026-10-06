---
"@pnpm/fs.packlist": patch
"@pnpm/store.cafs": patch
"pnpm": patch
---

Prevent bundled dependencies from traversing escaping directory symlinks or packaging files outside the package root [pnpm/tasks#83](https://github.com/pnpm/tasks/issues/83) [pnpm/tasks#93](https://github.com/pnpm/tasks/issues/93).
