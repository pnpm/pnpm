---
"pacquet": patch
---

A `file:` dependency declared by a package that was itself installed from a local directory is now resolved relative to that package's directory, not to the importer's [#13323](https://github.com/pnpm/pnpm/issues/13323). Installing a project whose local dependency depends on a sibling directory (`file:../child`) no longer fails with `Could not install from "…" as it does not exist`, and the snapshot entry for such a dependency is now written as `file:<path>` instead of `<name>@file:<path>`, matching the lockfile pnpm writes.
