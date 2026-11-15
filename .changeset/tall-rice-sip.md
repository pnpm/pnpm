---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

pnpm stops scripts and their descendants when it is killed during script startup [pnpm/tasks#313](https://github.com/pnpm/tasks/issues/313).
