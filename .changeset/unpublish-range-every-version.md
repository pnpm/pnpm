---
"@pnpm/registry-access.commands": patch
"pnpm": patch
"pacquet": patch
---

When the version or range given to `pnpm unpublish` matches every published version, the error now says so and shows the same command with `--force`. It suggested running `pnpm unpublish <pkg>@<version>`, the command that had just failed.
