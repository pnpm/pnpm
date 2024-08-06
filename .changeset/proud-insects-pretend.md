---
"@pnpm/config": minor
"pnpm": minor
---

Added pnpm version management to pnpm. If the `manage-package-manager-versions` setting is set to `true`, pnpm will switch to the version specified in the `packageManager` field of `package.json` [#8363](https://github.com/pnpm/pnpm/pull/8363). This is the same field used by Corepack. Example:

```json
{
  "packageManager": "pnpm@9.3.0"
}
```
