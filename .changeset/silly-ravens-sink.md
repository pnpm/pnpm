---
"@pnpm/config": minor
"@pnpm/normalize-registries": minor
"@pnpm/npm-resolver": minor
"@pnpm/parse-wanted-dependency": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/which-version-is-pinned": minor
"pnpm": minor
---

Add support for JSR package shorthands. User can now write `"@foo/bar": "jsr:^0.1.2"` (or `"@foo/bar": "jsr:@foo/bar@^0.1.2"`) in place of `"@foo/bar": "npm:@jsr/foo__bar@^0.1.2"` when using the JSR registry [#8941](https://github.com/pnpm/pnpm/issues/8941).

The `@jsr` scope also defaults to https://npm.jsr.io/ if `@jsr:registry` isn't set.
