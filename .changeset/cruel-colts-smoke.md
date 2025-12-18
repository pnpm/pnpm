---
"@pnpm/config": major
"pnpm": major
---

Replace workspace project specific `.npmrc` with `projectConfigs` in `pnpm-workspace.yaml`.

A workspace manifest with `projectConfigs` would look something like this:

```yaml
# File: pnpm-workspace.yaml
packages:
  - 'packages/*'
projectConfigs:
  'project-1':
    saveExact: true
  'project-2':
    savePrefix: '~'
```

Or this:

```yaml
# File: pnpm-workspace.yaml
packages:
  - 'packages/*'
projectConfigs:
  - match: ['project-1', 'project-2']
    modulesDir: 'node_modules'
    saveExact: true
```
