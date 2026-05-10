---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fix `pnpm upgrade --interactive --latest -r` not respecting named catalog groups. Previously, upgrading a dependency using a named catalog (e.g. `"catalog:foo"`) would incorrectly rewrite `package.json` to `"catalog:"` and place the updated version in the default catalog instead of the named one [#10115](https://github.com/pnpm/pnpm/issues/10115).
