---
"@pnpm/types": major
"@pnpm/config": major
"@pnpm/core": major
"@pnpm/headless": major
"@pnpm/builder.policy": major
"@pnpm/exec.build-commands": major
"@pnpm/plugin-commands-installation": major
"@pnpm/plugin-commands-rebuild": major
"@pnpm/workspace.manifest-writer": major
"pnpm": major
---

Remove deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies`.

Use the `allowBuilds` setting instead. It is a map where keys are package name patterns and values are booleans:
- `true` means the package is allowed to run build scripts
- `false` means the package is explicitly denied from running build scripts

Before:
```yaml
pnpm:
  onlyBuiltDependencies:
    - electron
  onlyBuiltDependenciesFile: 'allowed-builds.json'
  neverBuiltDependencies:
    - core-js
  ignoredBuiltDependencies:
    - esbuild
```

After:
```yaml
allowBuilds:
  electron: true
  core-js: false
  esbuild: false
```
