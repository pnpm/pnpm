---
"supi": patch
---

Allow to specify the overriden dependency's parent package.

For example, if `foo` should be overriden only in dependencies of bar v2, this configuration may be used:

```json
{
  ...
  "pnpm": {
    "overriden": {
      "bar@2>foo": "1.0.0"
    }
  }
}
```
