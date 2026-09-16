---
"pacquet": patch
---

Sped up `pnpm install` in Python workspaces with many projects. Projects now prepare concurrently, and projects with identical registry requirements share fresh dependency resolutions [pnpm/pnpm#14945](https://github.com/pnpm/pnpm/issues/14945).
