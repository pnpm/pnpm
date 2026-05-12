---
"@pnpm/global.commands": patch
"pnpm": patch
---

fix(global): preserve other members of an isolated install group when re-adding one of them.

When a global install group contains multiple packages, running `pnpm -g add <pkg>` on any single member used to remove the entire group and replace it with a fresh install of only the requested package, silently dropping the rest. The group's other members are now carried over at their current versions, so only the requested package is updated.
