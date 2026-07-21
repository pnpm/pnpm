---
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pnpm": minor
---

Added an `update` settings section to `pnpm-workspace.yaml`, superseding `updateConfig`. Its `ignore` field lists package name patterns that `pnpm update` and `pnpm outdated` should skip:

```yaml
update:
  ignore:
    - webpack
    - "@babel/*"
```

The old `updateConfig.ignoreDependencies` setting keeps working and will be supported until the next major version. If both `update` and `updateConfig` are set, `update` takes precedence and a warning is printed.
