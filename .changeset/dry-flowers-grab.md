---
"@pnpm/core": minor
"pnpm": minor
---

A new setting supported in the `pnpm` section of the `package.json` file [#4001](https://github.com/pnpm/pnpm/issues/4001). `onlyBuiltDependencies` is an array of package names that are allowed to be executed during installation. So if they have "preinstall", "install", or "postinstall" scripts, pnpm will run them during installation. E.g.:

```json
{
  "pnpm": {
    "onlyBuiltDependencies": ["fsevents"]
  }
}
```
