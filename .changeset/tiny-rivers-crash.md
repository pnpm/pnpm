---
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.package-requester": minor
"@pnpm/store.controller-types": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Added support for `trustPolicyExclude` [#10164](https://github.com/pnpm/pnpm/issues/10164).

You can now list one or more specific packages or versions that pnpm should allow to install, even if those packages don't satisfy the trust policy requirement. For example:

```yaml
trustPolicy: no-downgrade
trustPolicyExclude:
  - chokidar@4.0.3
  - webpack@4.47.0 || 5.102.1
```
