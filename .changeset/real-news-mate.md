---
"@pnpm/core": minor
"pnpm": minor
---

The `pnpm.peerDependencyRules.ignoreMissing` setting may accept package name patterns. So you may ignore any missing `@babel/*` peer dependencies, for instance:

```json
{
  "pnpm": {
    "peerDependencyRules": {
      "ignoreMissing": ["@babel/*"]
    }
  }
}
```
