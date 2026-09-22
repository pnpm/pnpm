---
"@pnpm/workspace.commands": patch
"pacquet": patch
"pnpm": patch
---

Fixed `pnpm version` failing on projects using a `package.yaml` manifest.

Fixed `pnpm init` creating an extra `package.json` when `package.yaml` is already present.
