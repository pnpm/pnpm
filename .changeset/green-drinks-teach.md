---
"@pnpm/core": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When `auto-install-peers` is set to `true`, automatically install direct peer dependencies [#5028](https://github.com/pnpm/pnpm/pull/5067).

So if your project the next manifest:

```json
{
  "dependencies": {
    "lodash": "^4.17.21"
  },
  "peerDependencies": {
    "react": "^18.2.0"
  }
}
```

pnpm will install both lodash and react as a regular dependencies.
