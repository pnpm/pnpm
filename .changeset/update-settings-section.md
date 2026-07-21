---
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pnpm": minor
"pacquet": patch
---

Added `update` and `audit` settings sections to `pnpm-workspace.yaml`, superseding the awkwardly named `updateConfig`, `auditConfig`, and top-level `auditLevel` settings:

```yaml
update:
  ignore: # was updateConfig.ignoreDependencies
    - webpack
    - "@babel/*"

audit:
  level: high # was auditLevel
  ignore: # was auditConfig.ignoreGhsas
    - GHSA-xxxx-yyyy-zzzz
```

`update.ignore` lists package name patterns that `pnpm update` and `pnpm outdated` should skip. `audit.level` and `audit.ignore` tune `pnpm audit`.

The deprecated `updateConfig`, `auditConfig`, and `auditLevel` settings keep working until the next major version. When both a new section value and its deprecated counterpart are set, the new section takes precedence and a warning is printed. Both the TypeScript CLI and the Rust config surface (pacquet) recognize the new sections.
