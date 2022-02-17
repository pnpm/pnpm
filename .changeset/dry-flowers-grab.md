---
"pnpm": minor
---

A new setting is supported in the `pnpm` section of the `package.json` file [#4001](https://github.com/pnpm/pnpm/issues/4001). `onlyBuiltDependencies` is an array of package names that are allowed to be executed during installation. If this field exists, only mentioned packages will be able to run install scripts.

```json
{
  "pnpm": {
    "onlyBuiltDependencies": ["fsevents"]
  }
}
```
