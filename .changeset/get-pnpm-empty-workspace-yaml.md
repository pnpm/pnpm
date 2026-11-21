---
"get-pnpm": patch
---

`npx get-pnpm` now reads a `pnpm-workspace.yaml` or `package.yaml` that holds only blank lines or comments as empty. It failed with "expected a document, but the input is empty".
