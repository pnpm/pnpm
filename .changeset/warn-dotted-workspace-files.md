---
"@pnpm/find-workspace-dir": patch
"pnpm": patch
---

Added `.pnpm-workspace.yaml` and `.pnpm-workspace.yml` to the list of invalid workspace manifest filenames that trigger a helpful error message [#10313](https://github.com/pnpm/pnpm/issues/10313).
