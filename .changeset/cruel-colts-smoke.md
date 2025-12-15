---
"@pnpm/config": major
"pnpm": major
---

Replace workspace project specific `.npmrc` with `projectSettings` in `pnpm-workspace.yaml`.

A workspace manifest with `projectSettings` would look something like this:

```yaml
# File: pnpm-workspace.yaml
packages:
  - 'packages/*'
projectSettings:
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
projectSettings:
  - match:
      - 'project-1'
      - 'project-2'
    settings:
      saveExact: true
```
