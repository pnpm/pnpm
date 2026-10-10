---
"pacquet": patch
---

pnpm now switches to the pnpm version a project pins even when it cannot read its configuration. Previously, a setting it rejected in the global config file, an `.npmrc` file, or `_auth`, or a `pnpm-workspace.yaml` it could not parse, stopped every command, `pnpm --version` included.
