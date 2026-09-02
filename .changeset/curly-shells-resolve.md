---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

Resolve path-like `scriptShell` values from `pnpm-workspace.yaml` against the workspace root so lifecycle scripts work from nested packages. Bare shell command names and absolute paths keep their existing resolution behavior. See https://github.com/pnpm/pnpm/issues/14422.
