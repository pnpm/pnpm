---
"@pnpm/resolve-dependencies": minor
"@pnpm/outdated": minor
"@pnpm/core": minor
"pnpm": minor
---

Added support for exact versions in `minimumReleaseAgeExclude` [#9985](https://github.com/pnpm/pnpm/issues/9985).

You can now list one or more specific versions that pnpm should allow to install, even if those versions don’t satisfy the maturity requirement set by `minimumReleaseAge`. For example:

```yaml
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
  - nx@21.6.5
  - webpack@4.47.0 || 5.102.1
```
