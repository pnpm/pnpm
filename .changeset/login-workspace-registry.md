---
"@pnpm/config.reader": patch
"@pnpm/registry-access.commands": patch
"pnpm": patch
---

Fixed `pnpm login` and `pnpm logout` ignoring `registries.default` from `pnpm-workspace.yaml` [#10099](https://github.com/pnpm/pnpm/issues/10099).
