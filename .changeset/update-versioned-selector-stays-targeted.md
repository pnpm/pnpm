---
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm update <pkg>@<version>` now updates only the selected packages and leaves unrelated dependencies unchanged. A selector that renames the package it installs — `pnpm update <alias>@npm:<pkg>@<version>` or the `jsr:` equivalent — now targets the package the alias installs rather than the alias.
