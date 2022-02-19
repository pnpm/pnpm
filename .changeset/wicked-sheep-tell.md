---
"@pnpm/core": minor
"pnpm": minor
---

When adding a new dependency, use the version specifier from the overrides, when present [#4313](https://github.com/pnpm/pnpm/issues/4313).

Normally, if the latest version of `foo` is `2.0.0`, then `pnpm add foo` installs `foo@^2.0.0`. This behavior changes if `foo` is specified in an override:

```json
{
  "pnpm": {
    "overrides": {
      "foo": "1.0.0"
    }
  }
}
```

In this case, `pnpm add foo` will add `foo@1.0.0` to the dependency. However, if a version is explicitly specifying, then the specified version will be used and the override will be ignored. So `pnpm add foo@0` will install v0 and it doesn't matter what is in the overrides.

