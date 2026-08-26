---
"pacquet": patch
---

`pnpm config delete` no longer leaves a blank line at the end of `pnpm-workspace.yaml` when it removes the last setting in the file. Because that blank line stayed behind, a later `pnpm config set` separated its new setting from it and the file ended up with two blank lines before the added setting.
