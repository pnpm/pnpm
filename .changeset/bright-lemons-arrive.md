---
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/types": minor
"pnpm": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/lockfile-types": minor
"@pnpm/lifecycle": minor
"@pnpm/plugin-commands-installation": minor
---

Dependencies patching is possible via the `pnpm.patchedDependencies` field of the `package.json`.
To patch a package, the package name, exact version, and the relative path to the patch file should be specified. For instance:

```json
{
  "pnpm": {
    "patchedDependencies": {
      "eslint@1.0.0": "./patches/eslint@1.0.0.patch"
    }
  }
}
```
