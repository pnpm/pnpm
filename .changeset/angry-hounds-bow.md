---
"@pnpm/core": minor
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/types": minor
"pnpm": minor
---

A new setting supported in the pnpm section of the `package.json` file: `allowNonAppliedPatches`. When it is set to `true`, non-applied patches will not cause an error, just a warning will be printed. For example:

```json
{
  "name": "foo",
  "version": "1.0.0",
  "pnpm": {
    "patchedDependencies": {
      "express@4.18.1": "patches/express@4.18.1.patch"
    },
    "allowNonAppliedPatches": true
  }
}
```

