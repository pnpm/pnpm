---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

Fix headless install not being used when a project has an injected self-referencing `file:` dependency that resolves to `link:` in the lockfile.
