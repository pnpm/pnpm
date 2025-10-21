---
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/headless": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/build-modules": minor
"@pnpm/core": minor
"@pnpm/builder.policy": minor
"@pnpm/types": minor
---

You can now allow specific versions of dependencies to run postinstall scripts. `onlyBuiltDependencies` now accepts package names with lists of trusted versions. For example:

```yaml
onlyBuiltDependencies:
  - nx@21.6.4 || 21.6.5
  - esbuild@0.25.1
```

Related PR: [#10104](https://github.com/pnpm/pnpm/pull/10104).
