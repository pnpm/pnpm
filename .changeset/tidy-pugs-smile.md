---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update <name>@<version>` now fails with `ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP` when the package is not a direct dependency of any selected project, instead of quietly updating it to whatever a fresh install would resolve. There is nowhere to record the version in that case, so the request cannot be honored, and the error points at the `overrides` entry that does pin a transitive dependency. Ranges and tags are unaffected, and a package that any selected project declares directly still takes its version as before.
