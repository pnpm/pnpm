---
"@pnpm/installing.linking.real-hoist": patch
"pnpm": patch
---

Fixed hoisted installs omitting a dependency that a package also declares as a required peer [#18193](https://github.com/pnpm/pnpm/issues/18193).
