---
"@pnpm/core": minor
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"pnpm": minor
---

A new setting is supported for ignoring specific deprecation messages: `pnpm.allowedDeprecatedVersions`. The setting should be provided in the `pnpm` section of the root `package.json` file. The below example will mute any deprecation warnings about the `request` package and warnings about `express` v1:

```json
{
  "pnpm": {
    "allowedDeprecatedVersions": {
      "request": "*",
      "express": "1"
    }
  }
}
```

Related issue: [#4306](https://github.com/pnpm/pnpm/issues/4306)
Related PR: [#4864](https://github.com/pnpm/pnpm/pull/4864)
