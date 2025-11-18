---
"@pnpm/plugin-commands-publishing": minor
"@pnpm/tools.plugin-commands-self-updater": minor
"@pnpm/tools.npm-manager": minor
"@pnpm/tools.installer": minor
"@pnpm/lockfile.types": minor
"@pnpm/types": minor
"@pnpm/config": minor
"@pnpm/read-project-manifest": minor
"@pnpm/manifest-utils": minor
"pnpm": minor
---

Added support for `devEngines.packageManager` with automatic npm download for publish command [#9812](https://github.com/pnpm/pnpm/issues/9812).

You can specify which npm version to use in your `package.json`:

```json
{
  "devEngines": {
    "packageManager": [
      {
        "name": "pnpm",
        "version": "^10.0.0"
      },
      {
        "name": "npm",
        "version": "10.2.3",
        "onFail": "download"
      }
    ]
  }
}
```

When `onFail: "download"` is set for npm, pnpm will automatically download and use the specified npm version for publishing. This enables using newer npm features like OIDC publishing without requiring users to manually install specific npm versions globally.

Note: Both pnpm and npm should be specified in the `packageManager` array when using this feature.
