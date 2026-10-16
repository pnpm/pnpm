---
"@pnpm/pkg-manifest.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pkg fix` now converts a `repository` string such as `github:pnpm/example` to the object form, as `npm pkg fix` does. It also normalizes a GitHub, GitLab, or Bitbucket repository URL to the `git+https://` or `git+ssh://` form [pnpm/pnpm#16980](https://github.com/pnpm/pnpm/issues/16980).
