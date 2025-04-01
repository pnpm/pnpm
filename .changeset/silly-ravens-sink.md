---
"@pnpm/npm-resolver": minor
"pnpm": minor
---

Add support for JSR package shorthands. User can now write `"@foo/bar": "jsr:^0.1.2"` in place of `"@foo/bar": "npm:@jsr/foo__bar@^0.1.2"` when using the JSR registry [#8941](https://github.com/pnpm/pnpm/issues/8941).

> [!NOTE]
> It only works when `.npmrc` defines a `@jsr` scope, for example:
>
> ```ini
> @jsr:registry=https://npm.jsr.io
> ```
