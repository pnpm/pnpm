# @pnpm/eslint-config

> pnpm's ESLint configuration

## Installation

```
pnpm add -D @pnpm/eslint-config eslint
```

## Usage

Create an `eslint.config.mjs` file:

```js
import eslintConfig from '@pnpm/eslint-config'

export default eslintConfig
```

The configuration includes TypeScript ports of the [perfectionist](https://github.com/KSXGitHub/perfectionist) rules that pnpm's Rust code is linted with. They are registered under the `perfectionist/` prefix, and each keeps the name of its Rust counterpart. The limits on function size and shape are also exported as `sizeAndShapeRules`, so a project can turn them off for code that does not meet them yet.

## License

MIT
