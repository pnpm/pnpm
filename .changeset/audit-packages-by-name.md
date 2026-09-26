---
"pacquet": minor
---

`pnpm audit <pkg>[@<version>]` audits the named packages and their dependencies without a project, so a package can be checked before it is installed. It resolves each package the way `pnpm add` would and leaves the current directory untouched [pnpm/pnpm#5174](https://github.com/pnpm/pnpm/issues/5174).
