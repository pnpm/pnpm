---
"pacquet": minor
---

A `registries` entry can now set `networkConcurrency`, the most requests pnpm keeps in flight to that registry's host. Requests to other registries keep the overall limit. The setting may live in `pnpm-workspace.yaml` or the global `config.yaml`.

```yaml
registries:
  https://npm.corp.example.com/:
    scopes: ["@acme"]
    networkConcurrency: 4
```
