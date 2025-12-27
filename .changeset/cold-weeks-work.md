---
"@pnpm/workspace.manifest-writer": minor
"@pnpm/types": minor
"@pnpm/config": minor
"pnpm": minor
---

Added support for `allowBuilds`, which is a new field that can be used instead of `onlyBuiltDependencies` and `ignoredBuiltDependencies`. The new `allowBuilds` field in your `pnpm-workspace.yaml` uses a map of package matchers to explicitly allow (`true`) or disallow (`false`) script execution. This allows for a single, easy-to-manage source of truth for your build permissions.

**Example Usage.** To explicitly allow all versions of `esbuild` to run scripts and prevent `core-js` from running them:

```yaml
allowBuilds:
  esbuild: true
  core-js: false
```

The example above achieves the same result as the previous configuration:

```yaml
onlyBuiltDependencies:
  - esbuild
ignoredBuiltDependencies:
  - core-js
```

Related PR: [#10311](https://github.com/pnpm/pnpm/pull/10311)
