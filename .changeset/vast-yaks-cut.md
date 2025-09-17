---
"pnpm": patch
"@pnpm/resolve-dependencies": patch
---

`minimumReleaseAgeExclude` config support patterns. For instance:

```yaml
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
  - '@eslint/*'
```

Related PR: [#9984](https://github.com/pnpm/pnpm/pull/9984).
