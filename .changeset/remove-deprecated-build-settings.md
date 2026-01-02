---
"pnpm": major
---

Remove deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies`.

Use the `allowBuilds` setting instead. It is a map where keys are package name patterns and values are booleans:
- `true` means the package is allowed to run build scripts
- `false` means the package is explicitly denied from running build scripts

Same as before, by default, none of the packages in the dependencies are allowed to run scripts. If a package has postinstall scripts and it isn't declared in `allowBuilds`, an error is printed.

Before:
```yaml
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
