---
"@pnpm/core": minor
"@pnpm/types": minor
"pnpm": minor
---

A new setting added: `pnpm.peerDependencyRules.allowAny`. `allowAny` is an array of package name patterns, any peer dependency matching the pattern will be resolved from any version, regardless of the range specified in `peerDependencies`. For instance:

```
{
  "pnpm": {
    "peerDependencyRules": {
      "allowAny": ["@babel/*", "eslint"]
    }
  }
}
```

The above setting will mute any warnings about peer dependency version mismatches related to `@babel/` packages or `eslint`.
