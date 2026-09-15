---
"pacquet": patch
---

`pnpm add <git repository>` now names a repository that ships no `package.json` after its owner and repository, such as `@owner/repo`. Two repositories that share a repository name can be dependencies of one project [pnpm/pnpm#14870](https://github.com/pnpm/pnpm/issues/14870).
