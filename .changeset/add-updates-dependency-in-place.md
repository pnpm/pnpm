---
"pacquet": patch
---

`pnpm add <pkg>` without a `--save-*` flag now updates an already-declared dependency in the group it occupies (`devDependencies` / `optionalDependencies`), matching pnpm, instead of always saving it into `dependencies`.
