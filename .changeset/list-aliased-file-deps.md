---
"pacquet": patch
---

`pnpm list` no longer adds an `npm:` prefix to aliased local `file:` directory dependencies, and `pnpm list --json` preserves `<name>@file:<path>` in their `version` field.
