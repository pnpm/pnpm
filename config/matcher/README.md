# @pnpm/matcher

> A simple pattern matcher for pnpm

## Install

```
pnpm add @pnpm/matcher
```

## Usage

```ts
import matcher from '@pnpm/matcher'

const match = matcher(['eslint-*'])
match('eslint-plugin-foo')
//> true
```

## License

[MIT](LICENSE)
