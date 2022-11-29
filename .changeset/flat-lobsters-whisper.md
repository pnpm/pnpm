---
"@pnpm/plugin-commands-installation": minor
"pnpm": minor
---

Overrides may be defined as a reference to a spec for a direct dependency by prefixing the name of the package you wish the version to match with a `$`.

```json
{
  "dependencies": {
    "foo": "^1.0.0"
  },
  "overrides": {
    // the override is defined as a reference to the dependency
    "foo": "$foo",
    // the referenced package does not need to match the overridden one
    "bar": "$foo"
  }
}
```
