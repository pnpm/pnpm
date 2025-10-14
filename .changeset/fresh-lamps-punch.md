---
"@pnpm/plugin-commands-publishing": minor
"@pnpm/tools.plugin-commands-self-updater": minor
"@pnpm/tools.npm-manager": minor
"@pnpm/tools.installer": minor
"@pnpm/lockfile.types": minor
"@pnpm/types": minor
"@pnpm/config": minor
"pnpm": minor
---

Added npm version management for publish command [#9812](https://github.com/pnpm/pnpm/issues/9812).

```json
{
  "pnpm": {
    "npmVersion": "10.2.3"
  }
}
```

This enables using newer npm features like OIDC publishing without requiring users to manually install specific npm versions globally.
