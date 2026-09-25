---
"pacquet": patch
"@pnpm/cli.commands": patch
"pnpm": patch
---

Bash completion now completes script names that contain a colon, such as `pnpm run test:u` to `pnpm run test:unit` [pnpm/pnpm#5482](https://github.com/pnpm/pnpm/issues/5482).
