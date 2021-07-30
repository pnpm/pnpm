---
"@pnpm/plugin-commands-publishing": minor
---

By default, for portability reasons, no files except those listed in the bin field will be marked as executable in the resulting package archive. The executableFiles field lets you declare additional fields that must have the executable flag (+x) set even if they aren't directly accessible through the bin field.

```json
"publishConfig": {
  "executableFiles": [
    "./dist/shim.js",
  ]
}
```
