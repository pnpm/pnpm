---
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

Frozen installs accept injected workspace packages whose child dependencies use `link:` or a bare relative path [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).
