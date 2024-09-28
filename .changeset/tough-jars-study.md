---
"@pnpm/hooks.read-package-hook": minor
"pnpm": minor
---

Added the ability for `overrides` to remove dependencies by specifying `"-"` as the field value [#8572](https://github.com/pnpm/pnpm/issues/8572). For example, to remove `lodash` from the dependencies, use this configuration in `package.json`:

```json
{
  "pnpm": {
    "overrides": {
      "lodash": "-"
    }
  }
}
```

