---
"@pnpm/resolve-dependencies": patch
---

Do not skip a package's peer resolution if it was previously resolved w/o peer dependencies but in the new node it has peer dependencies.
