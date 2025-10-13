---
"@pnpm/plugin-commands-config": patch
"pnpm": patch
---

Fixed scoped registry keys (e.g., `@scope:registry`) being parsed as property paths in `pnpm config get` when `--location=project` is used [#9362](https://github.com/pnpm/pnpm/issues/9362).
