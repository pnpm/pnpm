---
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed installation and peer dependency reporting for packages named `constructor` or `toString`. Optional peers with these names are also recognized when declared only in `peerDependenciesMeta`.
