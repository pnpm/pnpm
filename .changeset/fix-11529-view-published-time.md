---
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
---

Added "published X ago by Y" information to the `pnpm view` command output, similar to `npm view`. This is useful when comparing against `minimumReleaseAge`.

For example, `pnpm view pnpm` now shows:

```
published 17 hours ago by GitHub Actions
```
