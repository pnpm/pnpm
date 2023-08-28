---
"@pnpm/npm-resolver": minor
---

Force resolving from npm when using `npm:` protocol.

Even if a package exists in local workspace with a matching version, it will be ignored
if the dependency explicitly uses the `npm:` protocol prefix.

```
{
  "name": "my-other-package",
  "dependencies": {
    "my-package-via-workspace": "workspace:my-package",
    "my-package-via-npm": "npm:my-package"
  }
}
```

Closes https://github.com/pnpm/pnpm/issues/6992
