---
"pacquet": patch
---

pnpm now switches to the pnpm version a project pins even when it cannot read its configuration. Previously, a setting it rejected in the global config file, an `.npmrc` file, or `_auth` stopped every command, `pnpm --version` included.
