---
"@pnpm/config": minor
"pnpm": minor
---

Added a new setting, `dangerouslyAllowAllBuilds`, for automatically running any scripts of dependencies without the need to approve any builds. It was already possible to allow all builds by adding this to `pnpm-workspace.yaml`:

```yaml
neverBuiltDependencies: []
```

`dangerouslyAllowAllBuilds` has the same effect but also allows to be set globally via:

```
pnpm config set dangerouslyAllowAllBuilds true
```

It can also be set when running a command:

```
pnpm install --dangerously-allow-all-builds
```
