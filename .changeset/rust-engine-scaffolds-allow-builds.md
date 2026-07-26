---
"pacquet": patch
---

An install that blocks a dependency's build scripts now appends a placeholder for it to `pnpm-workspace.yaml`, so approving or denying the build is an edit rather than writing the block by hand:

```yaml
allowBuilds:
  es5-ext: set this to true or false
```

A placeholder is not a decision — the build stays blocked until it is replaced with `true` or `false` — and an existing entry is never overwritten [#13315](https://github.com/pnpm/pnpm/issues/13315).
