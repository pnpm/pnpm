---
"@pnpm/workspace.project-manifest-writer": patch
"@pnpm/yaml.document-sync": patch
"pnpm": patch
---

Preserve comments and existing key order when updating `package.yaml`. New keys are appended to their mapping [pnpm/pnpm#2008](https://github.com/pnpm/pnpm/issues/2008).
