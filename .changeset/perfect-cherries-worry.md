---
"@pnpm/manifest-utils": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/which-version-is-pinned": major
"pnpm": minor
---

A new value `rolling` for option `save-workspace-protocol`. When selected, pnpm will save workspace versions using a rolling alias (e.g. `"foo": "workspace:^"`) instead of pinning the current version number (e.g. `"foo": "workspace:^1.0.0"`). Usage example:

```
pnpm --save-workspace-protocol=rolling add foo
```
