---
"pacquet": patch
---

`pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` now move a catalog entry's resolution to the requested version. Previously, when the catalog entry was a range that covered the requested version but resolved to a different one, the request was dropped silently: nothing was installed, nothing was written, and no error was raised.
